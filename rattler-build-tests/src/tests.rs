use crate::utils::{
    check_info, get_extracted_package, get_package, host_subdir, rattler, variant_hash,
};
use std::{collections::HashMap, path::Path};

pub enum TestFunction {
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

pub(crate) fn test_help() {
    let help_test = rattler()
        .with_args(["help"])
        .map(|out| out.stdout)
        .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
        .unwrap();
    assert!(help_test);
}

pub(crate) fn test_no_cmd() {
    let help_test = rattler()
        // no heap allocations happen here, ideally!
        .with_args(Vec::<&str>::new())
        .map(|out| out.stdout)
        .map(|s| s.starts_with(b"Usage: rattler-build [OPTIONS]"))
        .unwrap();
    assert!(help_test);
}

pub(crate) fn test_run_exports(recipes: &Path, tmp_dir: &Path) {
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

type Tests = Vec<(&'static str, TestFunction)>;
pub(crate) fn initialize() -> Tests {
    let mut tests = vec![];
    fn __append_test_recipe_temp(
        queue: &mut Tests,
        name: &'static str,
        test_fn: fn(&Path, &Path) -> (),
    ) {
        let test_fn: TestFunction = test_fn.into();
        queue.push((name, test_fn));
    }
    fn __append_test(queue: &mut Tests, name: &'static str, test_fn: fn() -> ()) {
        let test_fn: TestFunction = test_fn.into();
        queue.push((name, test_fn));
    }
    macro_rules! add_test {
        ($queue:expr, $($names:ident $(,)?)+) => {{
            $(
                __append_test($queue, stringify!($names), $names);
            )+
        }};
    }
    macro_rules! add_test_recipe_temp {
        ($queue:expr, $($names:ident $(,)?)+) => {{
            $(
                __append_test_recipe_temp($queue, stringify!($names), $names);
            )+
        }};
    }
    add_test!(&mut tests, test_help, test_no_cmd);
    add_test_recipe_temp!(
        &mut tests,
        test_run_exports,
        test_pkg_hash,
        test_license_glob,
        test_python_noarch,
        test_git_source,
        test_package_content_test_execution,
        test_test_execution,
        test_noarch_flask,
        test_files_copy,
        test_tar_source,
        test_zip_source,
    );
    tests
}
