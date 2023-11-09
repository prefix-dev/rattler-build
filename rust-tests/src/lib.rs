#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use rattler_package_streaming::read::extract_tar_bz2;
    use std::{
        collections::HashMap,
        ffi::OsStr,
        fs::File,
        path::{Path, PathBuf},
        process::{Command, Output, Stdio},
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
            let mut command = self._get_command();
            if matches!(self, RattlerBuild::WithCargo(_)) {
                // cargo runs with quite (-q) to ensure we don't mix any additional output from our side
                command.args(["run", "-q", "-p", "rattler-build", "--"]);
            };
            command.args(args);
            // command.stderr(Stdio::inherit()).stdout(Stdio::inherit());
            // this makes it easy to debug issues, consider using --nocapture to get output with test
            // println!(
            //     "{} {}",
            //     command.get_program().to_string_lossy(),
            //     command.get_args().map(|s| s.to_string_lossy()).join(" ")
            // );
            command.output()
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
        #[cfg(not(target_arch = "aarch64"))]
        return "osx-64";

        #[cfg(target_os = "macos")]
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
        /// delete temp dir after the fact
        /// consider removing when debugging tests
        fn drop(&mut self) {
            self.0.exists().then(|| {
                // _ = std::fs::remove_dir_all(&self.0);
            });
        }
    }

    /// doesn't correctly handle spaces within argument of args escape all spaces
    fn shx<'a>(src: impl AsRef<str>) -> Option<String> {
        let (prog, args) = src.as_ref().split_once(' ')?;
        Command::new(prog)
            .args(args.split(' '))
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
    }

    fn tmp() -> WithTemp {
        for i in 0.. {
            let p = std::env::temp_dir().join(format!("{i}"));
            if p.exists() {
                continue;
            }
            return p.into();
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
        PathBuf::from(shx("cargo locate-project --workspace -q --message-format=plain").unwrap())
            .parent()
            .expect("couldn't fetch workspace root")
            .join("test-data")
    }

    #[test]
    fn test_help() {
        let help_test = rattler()
            .with_args(["help"])
            .map(|out| out.stdout)
            .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
            .unwrap();
        assert!(help_test);
    }

    #[test]
    fn test_no_cmd() {
        let help_test = rattler()
            // no heap allocations happen here, ideally!
            .with_args(Vec::<&str>::new())
            .map(|out| out.stderr)
            .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
            .unwrap();
        assert!(help_test);
    }

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
        let package_path = glob::glob(&glob_str)
            .expect("bad glob")
            .next()
            .expect("no glob matches")
            .expect("bad entry");
        _ = std::env::set_current_dir(path);
        folder.as_ref().join(package_path)
    }

    fn get_extracted_package(folder: impl AsRef<Path>, glob_str: impl AsRef<str>) -> PathBuf {
        let package_path = get_package(folder.as_ref(), glob_str.as_ref().to_string());
        // println!("package_path = {}", package_path.display());
        let extract_path = folder.as_ref().join("extract");
        // println!("extract_path = {}", extract_path.display());
        let _exr = extract_tar_bz2(File::open(package_path).unwrap(), &extract_path)
            .expect("failed to extract tar to target dir");
        extract_path
    }

    fn variant_hash(src: String) -> String {
        use sha1::Digest;
        let mut hasher = sha1::Sha1::new();
        hasher.update(src);
        let hash = hasher.finalize();
        format!("h{hash:x}")[..8].to_string()
    }

    #[test]
    fn test_pkg_hash() {
        let tmp = tmp();
        let rattler_build =
            rattler().build::<_, _, &str>(recipes().join("pkg_hash"), tmp.as_dir(), None);
        assert!(rattler_build.is_ok());
        let pkg = get_package(tmp.as_dir(), "pkg_hash".to_string());
        // yes this was broken because in rust default formatting for map does include that one space in the middle!
        let expected_hash = variant_hash(format!("{{\"target_platform\": \"{}\"}}", host_subdir()));
        let pkg_hash = format!("pkg_hash-1.0.0-{expected_hash}_my_pkg.tar.bz2");
        let pkg = pkg.display().to_string();
        assert!(pkg.ends_with(&pkg_hash));
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

    fn check_info(folder: PathBuf, expected: PathBuf) {
        for f in ["index.json", "about.json", "link.json", "paths.json"] {
            let expected = expected.join(f);
            // println!("expected = {}", expected.display());
            let mut cmp: HashMap<String, serde_json::Value> =
                serde_json::from_slice(&std::fs::read(expected).unwrap()).unwrap();

            let actual_path = folder.join("info").join(f);
            assert!(actual_path.exists());
            // println!("actual = {}", actual_path.display());
            let actual: HashMap<String, serde_json::Value> =
                serde_json::from_slice(&std::fs::read(actual_path).unwrap()).unwrap();

            if f == "index.json" {
                cmp.insert("timestamp".to_string(), actual["timestamp"].clone());
            }
            if f == "paths.json" {
                let act_arr = actual["paths"].as_array().unwrap();
                let cmp_arr = cmp["paths"].as_array().unwrap();
                assert!(act_arr.len() == cmp_arr.len());
                for (i, p) in act_arr.iter().enumerate() {
                    let c = cmp_arr[i].as_object().unwrap();
                    let p = p.as_object().unwrap();
                    assert!(c["_path"] == p["_path"]);
                    assert!(c["path_type"] == p["path_type"]);
                    if p["_path"]
                        .as_str()
                        .map(|s| !s.contains("dist-info"))
                        .unwrap_or_default()
                    {
                        // TODO: figure out why this is not the same b/w expected and cmp
                        assert!(c["sha256"] == p["sha256"]);
                        assert!(c["size_in_bytes"] == p["size_in_bytes"]);
                    }
                }
            } else {
                if actual.ne(&cmp) {
                    panic!("Expected {f} to be {cmp:?} but was {actual:?}");
                }
            }
        }
    }

    #[test]
    fn test_python_noarch() {
        let tmp = tmp();
        let rattler_build =
            rattler().build::<_, _, &str>(recipes().join("toml"), tmp.as_dir(), None);
        assert!(rattler_build.is_ok());
        let pkg = get_extracted_package(tmp.as_dir(), "toml");
        assert!(pkg.join("info/licenses/LICENSE").exists());
        let installer = pkg.join("site-packages/toml-0.10.2.dist-info/INSTALLER");
        assert!(installer.exists());
        assert_eq!(std::fs::read_to_string(installer).unwrap().trim(), "conda");
        check_info(pkg, recipes().join("toml/expected"));
    }
}
