#![deny(dead_code)]

use rattler_package_streaming::read::extract_tar_bz2;
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex, OnceLock},
};

enum TestFunction {
    NoArg(fn() -> ()),
    RecipeTemp(fn(&Path, &Path) -> ()),
}
impl From<fn() -> ()> for TestFunction {
    fn from(value: fn() -> ()) -> Self {
        TestFunction::NoArg(value)
    }
}
impl From<fn(&Path, &Path) -> ()> for TestFunction {
    fn from(value: fn(&Path, &Path) -> ()) -> Self {
        TestFunction::RecipeTemp(value)
    }
}

type Tests = Arc<Mutex<Vec<(&'static str, TestFunction)>>>;
static TESTS: OnceLock<Tests> = OnceLock::new();
fn get_test_queue() -> Tests {
    TESTS
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}
fn __append_test_recipe_temp(name: &'static str, test_fn: fn(&Path, &Path) -> ()) {
    let test_fn: TestFunction = test_fn.into();
    let test_queue = get_test_queue();
    if let Ok(mut handle) = test_queue.lock() {
        handle.push((name, test_fn));
    };
}
fn __append_test(name: &'static str, test_fn: fn() -> ()) {
    let test_fn: TestFunction = test_fn.into();
    let test_queue = get_test_queue();
    if let Ok(mut handle) = test_queue.lock() {
        handle.push((name, test_fn));
    };
}
macro_rules! add_test {
    ($name:ident) => {{
        __append_test(stringify!($name), $name);
    }};
}
macro_rules! add_test_recipe_temp {
    ($name:ident) => {{
        __append_test_recipe_temp(stringify!($name), $name);
    }};
}
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
pub const fn host_subdir() -> &'static str {
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
fn shx(src: impl AsRef<str>) -> Option<String> {
    let (prog, args) = src.as_ref().split_once(' ')?;
    Command::new(prog)
        .args(args.split(' '))
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

fn rattler() -> RattlerBuild {
    if let Ok(path) = std::env::var("RATTLER_BUILD_PATH") {
        if let Some(ret) = RattlerBuild::with_binary(path) {
            return ret;
        }
    }
    RattlerBuild::with_cargo(".").unwrap()
}

fn test_help() {
    let help_test = rattler()
        .with_args(["help"])
        .map(|out| out.stdout)
        .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
        .unwrap();
    assert!(help_test);
}

fn test_no_cmd() {
    let help_test = rattler()
        // no heap allocations happen here, ideally!
        .with_args(Vec::<&str>::new())
        .map(|out| out.stdout)
        .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
        .unwrap();
    assert!(help_test);
}

fn test_run_exports(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("run_exports"), tmp_dir, None);
    // ensure rattler build succeeded
    assert!(rattler_build.is_ok());
    let pkg = get_extracted_package(tmp_dir, "run_exports_test");
    assert!(pkg.join("info/run_exports.json").exists());
    let actual_run_export: HashMap<String, Vec<String>> =
        serde_json::from_slice(&std::fs::read(pkg.join("info/run_exports.json")).unwrap()).unwrap();
    assert!(actual_run_export.contains_key("weak"));
    assert_eq!(actual_run_export.get("weak").unwrap().len(), 1);
    let x = &actual_run_export.get("weak").unwrap()[0];
    assert!(x.starts_with("run_exports_test ==1.0.0 h") && x.ends_with("_0"));
}

fn get_package(folder: impl AsRef<Path>, mut glob_str: String) -> PathBuf {
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

fn test_pkg_hash(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("pkg_hash"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    let pkg = get_package(tmp_dir, "pkg_hash".to_string());
    // yes this was broken because in rust default formatting for map does include that one space in the middle!
    let expected_hash = variant_hash(format!("{{\"target_platform\": \"{}\"}}", host_subdir()));
    let pkg_hash = format!("pkg_hash-1.0.0-{expected_hash}_my_pkg.tar.bz2");
    let pkg = pkg.display().to_string();
    assert!(pkg.ends_with(&pkg_hash));
}

fn test_license_glob(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("globtest"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    let pkg = get_extracted_package(tmp_dir, "globtest");
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

fn test_python_noarch(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("toml"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    let pkg = get_extracted_package(tmp_dir, "toml");
    assert!(pkg.join("info/licenses/LICENSE").exists());
    let installer = pkg.join("site-packages/toml-0.10.2.dist-info/INSTALLER");
    assert!(installer.exists());
    assert_eq!(std::fs::read_to_string(installer).unwrap().trim(), "conda");
    check_info(pkg, recipes.join("toml/expected"));
}

fn test_git_source(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("llamacpp"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    let pkg = get_extracted_package(tmp_dir, "llama.cpp");
    // this is to ensure that the clone happens correctly
    let license = pkg.join("info/licenses/LICENSE");
    assert!(license.exists());
    let src = std::fs::read_to_string(license).unwrap();
    assert!(src.contains(" Georgi "));
}

fn test_package_content_test_execution(recipes: &Path, tmp_dir: &Path) {
    // let rattler_build = rattler().build::<_, _, &str>(
    //     recipes().join("package-content-tests/rich-recipe.yaml"),
    //     tmp.as_dir(),
    //     None,
    // );
    // assert!(rattler_build.is_ok());
    // assert!(rattler_build.unwrap().status.success());

    // let rattler_build = rattler().build( recipes().join("package-content-tests/llama-recipe.yaml"),
    //     tmp.as_dir(),
    //     Some(recipes().join("package-content-tests/variant-config.yaml")),
    // );
    // assert!(rattler_build.is_ok());
    // assert!(rattler_build.unwrap().status.success());

    let rattler_build = rattler().build::<_, _, &str>(
        recipes.join("package-content-tests/recipe-test-succeed.yaml"),
        tmp_dir,
        None,
    );
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.success());

    let rattler_build = rattler().build::<_, _, &str>(
        recipes.join("package-content-tests/recipe-test-fail.yaml"),
        tmp_dir,
        None,
    );
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.code().unwrap() == 1);
}

fn test_test_execution(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(
        recipes.join("test-execution/recipe-test-succeed.yaml"),
        tmp_dir,
        None,
    );
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.success());

    let rattler_build = rattler().build::<_, _, &str>(
        recipes.join("test-execution/recipe-test-fail.yaml"),
        tmp_dir,
        None,
    );
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.code().unwrap() == 1);
}

fn test_noarch_flask(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("flask"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.success());

    let pkg = get_extracted_package(tmp_dir, "flask");
    // this is to ensure that the clone happens correctly
    let license = pkg.join("info/licenses/LICENSE.rst");
    assert!(license.exists());

    assert!(pkg.join("info/test/run_test.sh").exists());
    assert!(pkg.join("info/test/run_test.bat").exists());
    assert!(pkg.join("info/test/run_test.py").exists());
    assert!(pkg.join("info/test/test_time_dependencies.json").exists());
    // make sure that the entry point does not exist
    assert!(!pkg.join("python-scripts/flask").exists());

    assert!(pkg.join("info/link.json").exists())
}

fn test_files_copy(recipes: &Path, tmp_dir: &Path) {
    if cfg!(target_os = "windows") {
        return;
    }
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("test-sources"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.success());
}

fn test_tar_source(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("tar-source"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.success());
}

fn test_zip_source(recipes: &Path, tmp_dir: &Path) {
    let rattler_build = rattler().build::<_, _, &str>(recipes.join("zip-source"), tmp_dir, None);
    assert!(rattler_build.is_ok());
    assert!(rattler_build.unwrap().status.success());
}

fn init_tests() {
    add_test!(test_help);
    add_test!(test_no_cmd);
    add_test_recipe_temp!(test_run_exports);
    add_test_recipe_temp!(test_pkg_hash);
    add_test_recipe_temp!(test_license_glob);
    add_test_recipe_temp!(test_python_noarch);
    add_test_recipe_temp!(test_git_source);
    add_test_recipe_temp!(test_package_content_test_execution);
    add_test_recipe_temp!(test_test_execution);
    add_test_recipe_temp!(test_noarch_flask);
    add_test_recipe_temp!(test_files_copy);
    add_test_recipe_temp!(test_tar_source);
    add_test_recipe_temp!(test_zip_source);
}

fn get_target_dir() -> std::io::Result<PathBuf> {
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

fn set_env_without_override(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}

/// entrypoint for all tests
fn main() -> io::Result<()> {
    init_tests();
    fn test_data_dir() -> PathBuf {
        PathBuf::from(shx("cargo locate-project --workspace -q --message-format=plain").unwrap())
            .parent()
            .expect("couldn't fetch workspace root")
            .join("test-data")
    }
    let recipes_dir = test_data_dir().join("recipes");

    // build project
    shx("cargo build --release -p rattler-build");
    // use binary just built
    let binary = get_target_dir()?.join("release/rattler-build");
    set_env_without_override("RATTLER_BUILD_PATH", binary.to_str().unwrap());

    let queue = get_test_queue();
    // cleanup after all tests have successfully completed
    let mut temp_dirs = vec![];
    // set_env_without_override
    if let Ok(handle) = queue.lock() {
        for (name, f) in handle.iter() {
            match f {
                TestFunction::NoArg(f) => f(),
                TestFunction::RecipeTemp(f) => {
                    let tmp_dir = std::env::temp_dir().join(name);
                    _ = std::fs::remove_dir_all(&tmp_dir);
                    _ = std::fs::create_dir_all(&tmp_dir);
                    f(&recipes_dir, &tmp_dir);
                    temp_dirs.push(tmp_dir);
                }
            }
            println!("success - rust-tests::test::{name}");
        }
    };
    println!("All tests completed successfully");
    for tmp_dir in temp_dirs {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    Ok(())
}
