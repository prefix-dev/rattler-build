#[cfg(test)]
mod tests {
    use rattler_package_streaming::read::extract_tar_bz2;
    use std::{
        collections::HashMap,
        ffi::OsStr,
        fs::File,
        path::{Path, PathBuf},
        process::{Command, Output},
    };

    enum RattlerBuild {
        WithCargo(PathBuf),
        WithBinary(String),
    }
    impl RattlerBuild {
        fn with_cargo(path: impl AsRef<Path>) -> Option<Self> {
            path.as_ref()
                .exists()
                .then(|| Self::WithCargo(path.as_ref().to_path_buf()))
        }
        fn with_binary(path: impl AsRef<Path>) -> Option<Self> {
            path.as_ref()
                .exists()
                .then(|| Self::WithBinary(path.as_ref().display().to_string()))
        }
        fn _get_command(&self) -> Command {
            match self {
                RattlerBuild::WithCargo(path) => {
                    let mut c = Command::new("cargo");
                    c.current_dir(path);
                    c
                }
                RattlerBuild::WithBinary(binary) => Command::new(binary),
            }
        }
        fn build<K: AsRef<Path>, T: AsRef<Path>, N: AsRef<Path>>(
            &self,
            recipe: K,
            output_dir: T,
            variant_config: Option<N>,
        ) -> std::io::Result<Output> {
            let rs = recipe.as_ref().display().to_string();
            let od = output_dir.as_ref().display().to_string();
            let iter = [
                "build",
                "--recipe",
                rs.as_str(),
                "--output-dir",
                od.as_str(),
            ];
            if let Some(variant_config_path) = variant_config {
                self.with_args(iter.into_iter().chain([
                    "--variant-config",
                    variant_config_path.as_ref().display().to_string().as_str(),
                ]))
            } else {
                self.with_args(iter)
            }
        }
        fn with_args(
            &self,
            args: impl IntoIterator<Item = impl AsRef<OsStr>>,
        ) -> std::io::Result<Output> {
            self._get_command()
                // cargo runs with quite (-q) to ensure we don't mix any additional output from our side
                .args(["run", "-q", "-p", "rattler-build", "--"])
                .args(args)
                .output()
        }
    }

    #[allow(unreachable_code)]
    pub const fn host_subdir() -> &'static str {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "linux-aarch64";

        #[cfg(target_os = "linux")]
        #[cfg(not(target_arch = "aarch64"))]
        return "linux-aarch64";

        #[cfg(target_os = "macos")]
        return "osx-arm64";

        #[cfg(target_os = "macos")]
        #[cfg(not(target_arch = "aarch64"))]
        return "osx-arm64";

        #[cfg(target_os = "windows")]
        #[cfg(not(target_arch = "aarch64"))]
        return "win-64";

        panic!("Unsupported platform")
    }

    struct WithTemp(PathBuf);
    impl WithTemp {
        fn as_dir(&self) -> &Path {
            self.0.as_path()
        }
    }
    impl From<PathBuf> for WithTemp {
        fn from(value: PathBuf) -> Self {
            WithTemp(value)
        }
    }
    impl Drop for WithTemp {
        fn drop(&mut self) {
            self.0.exists().then(|| {
                _ = std::fs::remove_dir_all(&self.0);
            });
        }
    }

    fn tmp() -> WithTemp {
        for i in 0.. {
            let p: WithTemp = std::env::temp_dir().join(format!("{i}")).into();
            if p.as_dir().exists() {
                continue;
            }
            return p;
        }
        unreachable!("above is an infinite loop")
    }
    fn rattler() -> RattlerBuild {
        if let Ok(path) = std::env::var("RATTLER_BUILD_PATH") {
            if let Some(ret) = RattlerBuild::with_binary(path) {
                return ret;
            }
        }
        RattlerBuild::with_cargo(".").unwrap()
    }
    fn recipes() -> PathBuf {
        test_data_dir().join("recipes")
    }
    fn test_data_dir() -> PathBuf {
        std::env::current_dir()
            .expect("couldn't fetch current_dir")
            .join("test-data")
    }

    #[test]
    fn test_functionality() {
        let help_test = rattler()
            .with_args(["help"])
            // help writes to stderr
            .map(|out| out.stderr)
            .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
            .unwrap();
        assert!(help_test);
    }

    // #[test]
    // fn rattler_test_build() {
    //     let rattler_test = rattler()
    //         .with_args(["build", "--render-only", "--recipe"])
    //         // help writes to stderr
    //         .map(|out| out.stderr);
    //     // assert!(help_test);
    // }

    #[test]
    fn test_run_exports() {
        let recipes = recipes();
        let tmp = tmp();
        let rattler_build =
            rattler().build::<_, _, &str>(recipes.join("run_exports"), tmp.as_dir(), None);
        // ensure rattler build succeeded
        assert!(rattler_build.is_ok());
        let pkg = get_extracted_package(tmp.as_dir(), "run_exports_test");
        assert!(pkg.join("info/run_exports.json").exists());
        let actual_run_export: HashMap<String, Vec<String>> =
            serde_json::from_slice(&std::fs::read(pkg.join("info/run_exports.json")).unwrap())
                .unwrap();
        assert!(actual_run_export.contains_key("weak"));
        assert_eq!(actual_run_export.get("weak").unwrap().len(), 1);
        let x = &actual_run_export.get("weak").unwrap()[0];
        assert!(x.starts_with("run_exports_test ==1.0.0 h") && x.ends_with("_0"));
    }

    fn get_package(folder: impl AsRef<Path>, mut glob_str: String) -> PathBuf {
        if !glob_str.ends_with("tar.bz2") {
            glob_str.push_str("*.tar.bz2");
        }
        if !glob_str.contains("/") {
            glob_str = "**/".to_string() + glob_str.as_str();
        }
        let path = std::env::current_dir().unwrap();
        _ = std::env::set_current_dir(folder.as_ref());
        let package_path = glob::glob(&glob_str).unwrap().next().unwrap().unwrap();
        _ = std::env::set_current_dir(path);
        package_path
    }

    fn get_extracted_package(folder: impl AsRef<Path>, glob_str: impl AsRef<str>) -> PathBuf {
        let package_path = get_package(folder, glob_str.as_ref().to_string());
        let extract_path = package_path.join("extract");
        let _exr = extract_tar_bz2(File::open(package_path).unwrap(), &extract_path)
            .expect("failed to extract tar to target dir");
        extract_path
    }

    fn variant_hash(map: impl Into<HashMap<String, String>>) -> String {
        use sha1::Digest;
        let mut hasher = sha1::Sha1::new();
        hasher.update(serde_json::to_string(&map.into()).unwrap());
        let hash = hasher.finalize();
        format!("{hash:x}")[..7].to_string()
    }

    #[test]
    fn test_pkg_hash() {
        let tmp = tmp();
        let rattler_build =
            rattler().build::<_, _, &str>(recipes().join("pkg_hash"), tmp.as_dir(), None);
        assert!(rattler_build.is_ok());
        let pkg = get_package(tmp.as_dir(), "pkg_hash".to_string());
        let expected_hash =
            variant_hash([("target_platform".to_string(), host_subdir().to_string())]);
        assert!(pkg
            .display()
            .to_string()
            .ends_with(&format!("pkg_hash-1.0.0-{expected_hash}_my_pkg.tar.bz2")))
    }

    #[test]
    fn test_license_glob() {
        let tmp = tmp();
        let rattler_build =
            rattler().build::<_, _, &str>(recipes().join("globtest"), tmp.as_dir(), None);
        assert!(rattler_build.is_ok());
        let pkg = get_extracted_package(tmp.as_dir(), "globtest");
        assert!(pkg.join("info/licenses/LICENSE").exists());
        assert!(pkg.join("info/licenses/cmake/FindTBB.cmake").exists());
        assert!(pkg.join("info/licenses/docs/ghp_environment.yml").exists());
        assert!(pkg.join("info/licenses/docs/rtd_environment.yml").exists());
        // check total count of files
        // 4 + 2 folder = 6
        let path = std::env::current_dir().unwrap();
        _ = std::env::set_current_dir(pkg);
        let glen = glob::glob("info/licenses/**/*")
            .unwrap()
            .filter(|s| s.is_ok())
            .count();
        _ = std::env::set_current_dir(path);
        assert_eq!(glen, 6);
    }

    fn check_info(pkg: PathBuf, dir: PathBuf) {
        todo!()
    }

    #[test]
    fn test_python_noarch() {
        let tmp = tmp();
        let rattler_build =
            rattler().build::<_, _, &str>(recipes().join("toml"), tmp.as_dir(), None);
        assert!(rattler_build.is_ok());
        let pkg = get_extracted_package(tmp.as_dir(), "toml");
        assert!(pkg.join("info/licenses/LICENSE").exists());
        assert!(pkg
            .join("site-packages/toml-0.10.2.dist-info/INSTALLER")
            .exists());
        let installer = pkg.join("site-packages/toml-0.10.2.dist-info/INSTALLER");
        assert_eq!(std::fs::read_to_string(installer).unwrap().trim(), "conda");
        check_info(pkg, recipes().join("toml/expected"));
    }
}
