#[cfg(test)]
mod tests {
    use duct::cmd;
    use fs_err as fs;
    use rattler_package_streaming::read::extract_tar_bz2;
    use std::{
        collections::HashMap,
        ffi::{OsStr, OsString},
        fs::File,
        path::{Component, Path, PathBuf},
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

        fn build<K: AsRef<Path>, T: AsRef<Path>>(
            &self,
            recipe: K,
            output_dir: T,
            variant_config: Option<&str>,
            target_platform: Option<&str>,
        ) -> Output {
            let rs = recipe.as_ref().display().to_string();
            let od = output_dir.as_ref().display().to_string();
            let mut iter = vec![
                "--log-style=plain",
                "build",
                "--recipe",
                rs.as_str(),
                "--package-format=tarbz2",
                "--output-dir",
                od.as_str(),
            ];
            if let Some(target_platform) = target_platform {
                iter.push("--target-platform");
                iter.push(target_platform);
            }
            if let Some(variant_config_path) = variant_config {
                iter.push("--variant-config");
                iter.push(variant_config_path);
            }
            self.with_args(iter)
        }

        fn with_args(&self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
            let (command, dir, cmd_args) = match self {
                RattlerBuild::WithCargo(path) => (
                    "cargo",
                    Some(path),
                    vec!["run", "--release", "-q", "-p", "rattler-build", "--"],
                ),
                RattlerBuild::WithBinary(binary) => (binary.as_str(), None, vec![]),
            };

            let mut args_vec: Vec<OsString> = cmd_args.into_iter().map(OsString::from).collect();

            args_vec.extend(args.into_iter().map(|s| s.as_ref().to_os_string()));

            let mut expression = cmd(command, &args_vec).stderr_to_stdout().stdout_capture();

            if let Some(dir) = dir {
                expression = expression.dir(dir);
            }

            let output = expression
                .unchecked()
                .run()
                .expect("failed to execute rattler-build");

            let stdout = String::from_utf8(output.stdout.clone())
                .expect("Failed to convert output to UTF-8");

            println!(
                "Running: {} {}",
                command,
                args_vec
                    .iter()
                    .map(|s| s.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            );

            println!("{}", stdout);

            output
        }

        fn generate_recipe_pyproject<I: AsRef<Path>, O: AsRef<Path>>(
            &self,
            input: I,
            output: O,
        ) -> Output {
            let input_str = input.as_ref().display().to_string();
            let output_str = output.as_ref().display().to_string();
            let args = vec![
                "--log-style=plain",
                "generate-recipe",
                "pyproject",
                "-i",
                input_str.as_str(),
                "-o",
                output_str.as_str(),
            ];
            self.with_args(args)
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
        fn drop(&mut self) {
            // self.0.exists().then_some({
            //     _ = fs::remove_dir_all(&self.0);
            // });
        }
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

    fn tmp(s: impl AsRef<str>) -> WithTemp {
        let path = std::env::temp_dir().join(s.as_ref());
        _ = fs::create_dir_all(&path);
        for i in 0.. {
            let p = path.join(format!("{i}"));
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

    fn run_build_from_yaml_string(recipe_yaml_string: String) {
        // Create output directory
        let tmp = tmp("test_run_build_from_yaml_string");
        let output_dir = tmp.as_dir().join("output");

        // Create recipe directory
        let recipe_dir = tmp.as_dir().join("recipe");
        fs::create_dir_all(&recipe_dir).unwrap();

        // Write recipe.yaml file
        fs::write(recipe_dir.join("recipe.yaml"), recipe_yaml_string).unwrap();

        // Build with rattler-build
        let rattler_build = rattler().build(recipe_dir, output_dir, None, None);
        assert!(rattler_build.status.success());
    }

    #[test]
    fn test_help() {
        let help_test = rattler().with_args(["help"]);

        assert!(help_test.status.success());

        let help_test = help_test.stdout;
        let help_text = help_test.split(|c| *c == b'\n').collect::<Vec<_>>();

        #[cfg(target_family = "unix")]
        assert!(help_text[0].starts_with(b"Usage: rattler-build [OPTIONS]"));
        #[cfg(target_family = "windows")]
        assert!(help_text[0].starts_with(b"Usage: rattler-build.exe [OPTIONS]"));
    }

    #[test]
    fn test_no_cmd() {
        let help_text = rattler().with_args(Vec::<&str>::new());

        assert!(help_text.status.success());

        let help_text = help_text.stdout;
        let lines = help_text.split(|c| *c == b'\n').collect::<Vec<_>>();
        assert!(lines[0].starts_with(b"Usage: rattler-build [OPTIONS]"));
    }

    #[test]
    fn test_run_exports_from() {
        let recipes = recipes();
        let tmp = tmp("test_run_exports_from");
        let rattler_build =
            rattler().build::<_, _>(recipes.join("run_exports_from"), tmp.as_dir(), None, None);
        // ensure rattler build succeeded
        assert!(rattler_build.status.success());
        let pkg = get_extracted_package(tmp.as_dir(), "run_exports_test");
        assert!(pkg.join("info/run_exports.json").exists());
        let actual_run_export: HashMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(pkg.join("info/run_exports.json")).unwrap()).unwrap();
        assert!(actual_run_export.contains_key("weak"));
        assert_eq!(actual_run_export.get("weak").unwrap().len(), 1);
        let x = &actual_run_export.get("weak").unwrap()[0];
        assert!(x.starts_with("run_exports_test ==1.0.0 h") && x.ends_with("_0"));
        assert!(pkg.join("info/index.json").exists());
        let index_json: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&fs::read(pkg.join("info/index.json")).unwrap()).unwrap();
        assert!(!index_json.contains_key("depends"));
    }

    #[test]
    fn test_run_exports() {
        let recipes = recipes();
        let tmp = tmp("test_run_exports");
        let rattler_build = rattler().build(recipes.join("run_exports"), tmp.as_dir(), None, None);
        // ensure rattler build succeeded
        assert!(rattler_build.status.success());
        let pkg = get_extracted_package(tmp.as_dir(), "run_exports_test");
        assert!(pkg.join("info/run_exports.json").exists());
        let actual_run_export: HashMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(pkg.join("info/run_exports.json")).unwrap()).unwrap();
        assert!(actual_run_export.contains_key("weak"));
        assert_eq!(actual_run_export.get("weak").unwrap().len(), 1);
        let x = &actual_run_export.get("weak").unwrap()[0];
        assert!(x.starts_with("run_exports_test ==1.0.0 h") && x.ends_with("_0"));
        assert!(pkg.join("info/index.json").exists());
        let index_json: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&fs::read(pkg.join("info/index.json")).unwrap()).unwrap();
        assert!(!index_json.contains_key("depends"));
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

    #[test]
    fn test_pkg_hash() {
        let tmp = tmp("test_pkg_hash");
        let rattler_build = rattler().build(recipes().join("pkg_hash"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());

        let pkg = get_package(tmp.as_dir(), "pkg_hash".to_string());
        // yes this was broken because in rust default formatting for map does include that one space in the middle!
        let expected_hash = variant_hash(format!("{{\"target_platform\": \"{}\"}}", host_subdir()));
        let pkg_hash = format!("pkg_hash-1.0.0-{expected_hash}_my_pkg.tar.bz2");
        let pkg = pkg.display().to_string();
        assert!(pkg.ends_with(&pkg_hash));
    }

    #[test]
    fn test_license_glob() {
        let tmp = tmp("test_license_glob");
        let rattler_build = rattler().build(recipes().join("globtest"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());

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
        assert_eq!(glen, 8);
    }

    fn check_info(folder: PathBuf, expected: PathBuf) {
        for f in ["index.json", "about.json", "link.json", "paths.json"] {
            let expected = expected.join(f);
            // println!("expected = {}", expected.display());
            let mut cmp: HashMap<String, serde_json::Value> =
                serde_json::from_slice(&fs::read(expected).unwrap()).unwrap();

            let actual_path = folder.join("info").join(f);
            assert!(actual_path.exists());
            // println!("actual = {}", actual_path.display());
            let actual: HashMap<String, serde_json::Value> =
                serde_json::from_slice(&fs::read(actual_path).unwrap()).unwrap();

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
                panic!("Mismatch in {f}:\nExpected:\n  {cmp:?}{f}\nActual:\n  {actual:?}");
            }
        }
    }

    #[test]
    fn test_python_noarch() {
        let tmp = tmp("test_python_noarch");
        let rattler_build = rattler().build(recipes().join("toml"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());

        let pkg = get_extracted_package(tmp.as_dir(), "toml");
        assert!(pkg.join("info/licenses/LICENSE").exists());
        let installer = pkg.join("site-packages/toml-0.10.2.dist-info/INSTALLER");
        assert!(installer.exists());
        assert_eq!(fs::read_to_string(installer).unwrap().trim(), "conda");
        check_info(pkg, recipes().join("toml/expected"));
    }

    #[test]
    fn test_git_source() {
        let tmp = tmp("test_git_source");
        let rattler_build = rattler().build(recipes().join("llamacpp"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());

        let pkg = get_extracted_package(tmp.as_dir(), "llama.cpp");
        // this is to ensure that the clone happens correctly
        let license = pkg.join("info/licenses/LICENSE");
        assert!(license.exists());
        let src = fs::read_to_string(license).unwrap();
        assert!(src.contains(" Georgi "));
    }

    #[test]
    fn test_package_content_test_execution() {
        let tmp = tmp("test_package_content_test_execution");
        // let rattler_build = rattler().build(
        //     recipes().join("package-content-tests/rich-recipe.yaml"),
        //     tmp.as_dir(),
        //     None,
        // );
        //

        // assert!(rattler_build.status.success());

        // let rattler_build = rattler().build( recipes().join("package-content-tests/llama-recipe.yaml"),
        //     tmp.as_dir(),
        //     Some(recipes().join("package-content-tests/variant-config.yaml")),
        // );
        //

        // assert!(rattler_build.status.success());

        let rattler_build = rattler().build(
            recipes().join("package-content-tests/recipe-test-succeed.yaml"),
            tmp.as_dir(),
            None,
            None,
        );

        assert!(rattler_build.status.success());

        let rattler_build = rattler().build(
            recipes().join("package-content-tests/recipe-test-fail.yaml"),
            tmp.as_dir(),
            None,
            None,
        );

        assert!(rattler_build.status.code() == Some(1));
    }

    #[test]
    fn test_test_execution() {
        let tmp = tmp("test_test_execution");
        let rattler_build = rattler().build(
            recipes().join("test-execution/recipe-test-succeed.yaml"),
            tmp.as_dir(),
            None,
            None,
        );

        assert!(rattler_build.status.success());

        let rattler_build = rattler().build(
            recipes().join("test-execution/recipe-test-fail.yaml"),
            tmp.as_dir(),
            None,
            None,
        );

        assert!(rattler_build.status.code().unwrap() == 1);
    }

    #[test]
    fn test_files_copy() {
        if cfg!(target_os = "windows") {
            return;
        }
        let tmp = tmp("test-sources");
        let rattler_build =
            rattler().build(recipes().join("test-sources"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());
    }

    #[test]
    fn test_tar_source() {
        let tmp = tmp("test_tar_source");
        let rattler_build = rattler().build(recipes().join("tar-source"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());
    }

    #[test]
    fn test_zip_source() {
        let tmp = tmp("test_zip_source");
        let rattler_build = rattler().build(recipes().join("zip-source"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());
    }

    #[test]
    fn test_7z_source() {
        let tmp = tmp("test_7z_source");
        let rattler_build = rattler().build(recipes().join("7z-source"), tmp.as_dir(), None, None);

        assert!(rattler_build.status.success());
    }

    #[test]
    fn test_dry_run_cf_upload() {
        let tmp = tmp("test_polarify");
        let variant = recipes().join("polarify").join("linux_64_.yaml");
        let rattler_build = rattler().build(
            recipes().join("polarify"),
            tmp.as_dir(),
            variant.to_str(),
            None,
        );

        assert!(rattler_build.status.success());

        // try to upload the package using the rattler upload command
        let pkg_path = get_package(tmp.as_dir(), "polarify".to_string());
        let rattler_upload = rattler().with_args([
            "upload",
            "-vvv",
            "conda-forge",
            "--feedstock",
            "polarify",
            "--feedstock-token",
            "fake-feedstock-token",
            "--staging-token",
            "fake-staging-token",
            "--dry-run",
            pkg_path.to_str().unwrap(),
        ]);

        let output = String::from_utf8(rattler_upload.stdout).unwrap();
        assert!(rattler_upload.status.success());
        assert!(output.contains("Done uploading packages to conda-forge"));
    }

    #[test]
    fn test_correct_sha256() {
        let tmp = tmp("correct-sha");
        let rattler_build =
            rattler().build(recipes().join("correct-sha"), tmp.as_dir(), None, None);
        assert!(rattler_build.status.success());
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn test_rpath() {
        let tmp = tmp("test_rpath");
        let rattler_build = rattler().build(
            recipes().join("rpath"),
            tmp.as_dir(),
            None,
            Some("linux-64"),
        );

        assert!(rattler_build.status.success());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_overlinking_check() {
        let tmp = tmp("test_overlink_check");
        let rattler_build = rattler().build(
            recipes().join("overlinking"),
            tmp.as_dir(),
            None,
            Some("linux-64"),
        );
        assert!(!rattler_build.status.success());
        let output = String::from_utf8(rattler_build.stdout).unwrap();
        assert!(output.contains("linking check error: Overlinking against"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_overdepending_check() {
        let tmp = tmp("test_overdepending_check");
        let rattler_build = rattler().build(
            recipes().join("overdepending"),
            tmp.as_dir(),
            None,
            Some("linux-64"),
        );
        assert!(!rattler_build.status.success());
        let output = String::from_utf8(rattler_build.stdout).unwrap();
        assert!(output.contains("linking check error: Overdepending against"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_allow_missing_dso() {
        let tmp = tmp("test_allow_missing_dso");
        let rattler_build = rattler().build(
            recipes().join("allow_missing_dso"),
            tmp.as_dir(),
            None,
            Some("linux-64"),
        );
        assert!(rattler_build.status.success());
        let output = String::from_utf8(rattler_build.stdout).unwrap();
        assert!(output.contains("it is included in the allow list. Skipping..."));
    }

    #[test]
    fn test_render_only_recipe() {
        let recipe_path = recipes().join("rich").join("recipe.yaml");
        let rattler_build_render_only = rattler().with_args([
            "build",
            "--recipe",
            recipe_path.to_str().unwrap(),
            "--render-only",
        ]);

        assert!(rattler_build_render_only.status.success());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn render_only_dont_install_pkgs() {
        // test covering this issue https://github.com/prefix-dev/rattler-build/issues/716
        let tmp = tmp("test_render_only");

        let rattler_build = rattler().with_args([
            "build",
            "--recipe",
            recipes().join("flask").to_str().unwrap(),
            "--render-only",
            "--output-dir",
            tmp.as_dir().to_str().unwrap(),
            "--target-platform",
            "win-64",
        ]);

        assert!(rattler_build.status.success());
    }

    #[test]
    fn test_script_content_scalar_with_jinja() {
        // Content as scalar
        let recipe_content = r#"
package:
  name: hellopackage
  version: 1.0.0
build:
  script:
    content: ${{ PYTHON }} --help
requirements:
  host:
    - python
"#;
        run_build_from_yaml_string(recipe_content.to_string());
    }

    #[test]
    fn test_script_content_sequence_with_jinja() {
        // Content as sequence
        let recipe_content = r#"
package:
  name: hellopackage
  version: 1.0.0
build:
  script:
    content:
      - ${{ PYTHON }} --help
requirements:
  host:
    - python
"#;
        run_build_from_yaml_string(recipe_content.to_string());
    }

    #[test]
    fn test_missing_license_file() {
        let tmp = tmp("test_missing_license_file");
        let rattler_build = rattler().build(
            recipes().join("missing_license_file"),
            tmp.as_dir(),
            None,
            None,
        );

        assert!(!rattler_build.status.success());
        let output = String::from_utf8(rattler_build.stdout).unwrap();
        assert!(output.contains("No license files were copied"));
        assert!(output.contains("The following license files were not found: does-not-exist.txt"));
    }

    #[test]
    fn test_missing_license_glob() {
        let tmp = tmp("test_missing_license_glob");
        let rattler_build = rattler().build(
            recipes().join("missing_license_glob"),
            tmp.as_dir(),
            None,
            None,
        );

        assert!(!rattler_build.status.success());
        let output = String::from_utf8(rattler_build.stdout).unwrap();
        assert!(output.contains("No license files were copied"));
        assert!(output.contains("The following license files were not found: *.license"));
    }

    #[test]
    fn test_generate_recipe_pyproject_basic() {
        let tmp = tmp("test_generate_recipe_pyproject_basic");
        // Create the temp directory
        fs::create_dir_all(tmp.as_dir()).unwrap();

        // Create a basic pyproject.toml
        let pyproject_content = r#"
[project]
name = "test-package"
version = "1.0.0"
description = "A test package for pyproject recipe generation"
dependencies = [
    "requests>=2.25.0",
    "click>=8.0.0"
]

[project.scripts]
test-tool = "test_package.cli:main"

[build-system]
requires = ["setuptools", "wheel"]
build-backend = "setuptools.build_meta"
"#;

        let pyproject_path = tmp.as_dir().join("pyproject.toml");
        fs::write(&pyproject_path, pyproject_content).unwrap();

        let recipe_path = tmp.as_dir().join("recipe.yaml");

        // Run rattler-build generate-recipe pyproject
        let rattler_build = rattler().generate_recipe_pyproject(&pyproject_path, &recipe_path);

        assert!(
            rattler_build.status.success(),
            "Command failed: {}",
            String::from_utf8_lossy(&rattler_build.stdout)
        );

        // Check that recipe.yaml was created
        assert!(recipe_path.exists(), "recipe.yaml was not created");

        // Check recipe content
        let recipe_content = fs::read_to_string(&recipe_path).unwrap();

        // Should have schema header
        assert!(recipe_content.contains("# yaml-language-server: $schema="));
        assert!(recipe_content.contains("schema_version: 1"));

        // Should have correct package info
        assert!(recipe_content.contains("name: test-package"));
        assert!(recipe_content.contains("version: 1.0.0"));

        // Should have dependencies converted
        assert!(recipe_content.contains("requests >=2.25.0"));
        assert!(recipe_content.contains("click >=8.0.0"));

        // Should have entry points
        assert!(recipe_content.contains("test-tool = test_package.cli:main"));

        // Should have build system requirements
        assert!(recipe_content.contains("setuptools"));
        assert!(recipe_content.contains("wheel"));
    }

    #[test]
    fn test_generate_recipe_pyproject_with_conda_overrides() {
        let tmp = tmp("test_generate_recipe_pyproject_conda_overrides");
        // Create the temp directory
        fs::create_dir_all(tmp.as_dir()).unwrap();

        // Create a pyproject.toml with conda recipe overrides
        let pyproject_content = r#"
[project]
name = "advanced-package"
version = "2.0.0"
description = "An advanced package with conda overrides"
dependencies = [
    "numpy>=1.21.0"
]

[tool.conda.recipe]
schema_version = 2

[tool.conda.recipe.context]
custom_var = "custom_value"

[tool.conda.recipe.about]
license = "MIT"
homepage = "https://example.com"

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
"#;

        let pyproject_path = tmp.as_dir().join("pyproject.toml");
        fs::write(&pyproject_path, pyproject_content).unwrap();

        let recipe_path = tmp.as_dir().join("recipe.yaml");

        // Run rattler-build generate-recipe pyproject
        let rattler_build = rattler().generate_recipe_pyproject(&pyproject_path, &recipe_path);

        assert!(
            rattler_build.status.success(),
            "Command failed: {}",
            String::from_utf8_lossy(&rattler_build.stdout)
        );

        // Check recipe content
        let recipe_content = fs::read_to_string(&recipe_path).unwrap();

        // Should have custom schema version
        assert!(recipe_content.contains("schema_version: 2"));

        // Should have conda overrides applied
        assert!(recipe_content.contains("custom_var: custom_value"));
        assert!(recipe_content.contains("license: MIT"));
        assert!(recipe_content.contains("homepage: https://example.com"));

        // Should have hatchling build system
        assert!(recipe_content.contains("hatchling"));
    }
}
