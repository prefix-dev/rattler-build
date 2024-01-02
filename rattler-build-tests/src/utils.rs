use rattler_package_streaming::read::extract_tar_bz2;
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::File,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

#[derive(Debug)]
pub enum RattlerBuild {
    WithCargo(Command),
    WithBinary(Command),
}
impl RattlerBuild {
    fn with_cargo(path: impl AsRef<Path>) -> Option<Self> {
        let mut command = Command::new("cargo");
        path.as_ref().exists().then(|| {
            command.current_dir(path.as_ref());
            Self::WithCargo(command)
        })
    }
    fn with_binary(path: impl AsRef<Path>) -> Option<Self> {
        path.as_ref()
            .exists()
            .then(|| Self::WithBinary(Command::new(path.as_ref())))
    }
    fn _get_command(self) -> Command {
        match self {
            RattlerBuild::WithCargo(c) | RattlerBuild::WithBinary(c) => c,
        }
    }
    pub(crate) fn build<K: AsRef<Path>, T: AsRef<Path>, N: AsRef<Path>>(
        self,
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
    pub(crate) fn with_args(
        self,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> std::io::Result<Output> {
        let with_cargo = matches!(self, RattlerBuild::WithCargo(_));
        let mut command = self._get_command();
        if with_cargo {
            // cargo runs with quite (-q) to ensure we don't mix any additional output from our side
            command.args(["run", "--release", "-q", "-p", "rattler-build", "--"]);
        };
        command.args(args);
        // this makes it easy to debug issues, consider using --nocapture to get output with test
        // command
        //     .stderr(std::process::Stdio::inherit())
        //     .stdout(std::process::Stdio::inherit());
        // use itertools::Itertools;
        // println!(
        //     "{} {}",
        //     command.get_program().to_string_lossy(),
        //     command.get_args().map(|s| s.to_string_lossy()).join(" ")
        // );
        command.output()
    }
}

#[allow(unreachable_code)]
pub(crate) const fn host_subdir() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "linux-aarch64";

    #[cfg(target_os = "linux")]
    #[cfg(not(target_arch = "aarch64"))]
    return "linux-64";

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

/// doesn't correctly handle spaces within argument of args escape all spaces
pub(crate) fn shx(src: impl AsRef<str>) -> Option<String> {
    let (prog, args) = src.as_ref().split_once(' ')?;
    Command::new(prog)
        .args(args.split(' '))
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

pub(crate) fn rattler() -> RattlerBuild {
    match std::env::var("RATTLER_BUILD_PATH").map(RattlerBuild::with_binary) {
        Ok(Some(rattler_build)) => rattler_build,
        _ => RattlerBuild::with_cargo(".").unwrap(),
    }
}

pub(crate) fn get_package(folder: impl AsRef<Path>, mut glob_str: String) -> PathBuf {
    if !glob_str.ends_with("tar.bz2") {
        glob_str.push_str("*.tar.bz2");
    }
    if !glob_str.contains('/') {
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

pub(crate) fn get_extracted_package(
    folder: impl AsRef<Path>,
    glob_str: impl AsRef<str>,
) -> PathBuf {
    let package_path = get_package(folder.as_ref(), glob_str.as_ref().to_string());
    // println!("package_path = {}", package_path.display());
    let extract_path = folder.as_ref().join("extract");
    // println!("extract_path = {}", extract_path.display());
    let _exr = extract_tar_bz2(File::open(package_path).unwrap(), &extract_path)
        .expect("failed to extract tar to target dir");
    extract_path
}

pub(crate) fn variant_hash(src: String) -> String {
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(src);
    let hash = hasher.finalize();
    format!("h{hash:x}")[..8].to_string()
}

pub(crate) fn check_info(folder: PathBuf, expected: PathBuf) {
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
                let cpath = PathBuf::from(c["_path"].as_str().unwrap());
                let ppath = PathBuf::from(p["_path"].as_str().unwrap());
                assert!(cpath == ppath);
                assert!(c["path_type"] == p["path_type"]);
                if ppath
                    .components()
                    .any(|s| s.eq(&Component::Normal("dist-info".as_ref())))
                {
                    assert!(c["sha256"] == p["sha256"]);
                    assert!(c["size_in_bytes"] == p["size_in_bytes"]);
                }
            }
        } else if actual.ne(&cmp) {
            panic!("Expected {f} to be {cmp:?} but was {actual:?}");
        }
    }
}

pub(crate) fn get_target_dir() -> std::io::Result<PathBuf> {
    let output = shx("cargo metadata --no-deps").expect("Failed to run cargo metadata");
    let json: serde_json::Value = serde_json::from_str(output.as_str())?;
    json.get("target_directory")
        .and_then(|td| td.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Failed to find target_directory",
            )
        })
}

pub(crate) fn set_env_without_override(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}
