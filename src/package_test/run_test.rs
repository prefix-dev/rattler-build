//! Testing a package produced by rattler-build (or conda-build)
//!
//! Tests are part of the final package (under the `info/test` directory).
//! There are multiple test types:
//!
//! * `commands` - run a list of commands and check their exit code
//! * `imports` - import a list of modules and check if they can be imported
//! * `files` - check if a list of files exist

use fs_err as fs;
use rattler_conda_types::package::IndexJson;
use std::fmt::Write as fmt_write;
use std::{
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use dunce::canonicalize;
use rattler::package_cache::CacheKey;
use rattler_conda_types::{package::ArchiveIdentifier, MatchSpec, Platform};
use rattler_index::index;
use rattler_shell::{
    activation::{ActivationError, ActivationVariables, Activator},
    shell::{Shell, ShellEnum, ShellScript},
};

use crate::{
    env_vars,
    recipe::parser::{CommandsTestRequirements, PythonTest},
    render::solver::create_environment,
    tool_configuration,
};

#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum TestError {
    #[error("failed package content tests: {0}")]
    PackageContentTestFailed(String),

    #[error("failed package content tests: {0}")]
    PackageContentTestFailedStr(&'static str),

    #[error("failed to get environment `PREFIX` variable")]
    PrefixEnvironmentVariableNotFound,

    #[error("failed to build glob from pattern")]
    GlobError(#[from] globset::Error),

    #[error("failed to run test")]
    TestFailed,

    #[error("failed to read package: {0}")]
    PackageRead(#[from] std::io::Error),

    #[error("failed to write testing script: {0}")]
    FailedToWriteScript(#[from] std::fmt::Error),

    #[error("failed to parse MatchSpec: {0}")]
    MatchSpecParse(String),

    #[error("failed to setup test environment: {0}")]
    TestEnvironmentSetup(#[from] anyhow::Error),

    #[error("failed to setup test environment: {0}")]
    TestEnvironmentActivation(#[from] ActivationError),

    #[error("failed to parse JSON from test files: {0}")]
    TestJSONParseError(#[from] serde_json::Error),

    #[error("failed to parse MatchSpec from test files: {0}")]
    TestMatchSpecParseError(#[from] rattler_conda_types::ParseMatchSpecError),

    #[error("missing package file name")]
    MissingPackageFileName,

    #[error("archive type not supported")]
    ArchiveTypeNotSupported,

    #[error("could not determine target platform from package file (no index.json?)")]
    CouldNotDetermineTargetPlatform,
}

#[derive(Debug)]
enum Tests {
    Commands(PathBuf),
    Python(PathBuf),
}

fn run_in_environment(
    shell: ShellEnum,
    cmd: String,
    cwd: &Path,
    environment: &Path,
    build_environment: Option<PathBuf>,
) -> Result<(), TestError> {
    let current_path = std::env::var("PATH")
        .ok()
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>());

    // if we are in a conda environment, we need to deactivate it before activating the host / build prefix
    let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

    let activation_vars = ActivationVariables {
        conda_prefix,
        path: current_path,
        path_modification_behavior: Default::default(),
    };

    let host_prefix_activator =
        Activator::from_path(environment, shell.clone(), Platform::current())?;

    let host_activation = host_prefix_activator
        .activation(activation_vars)
        .expect("Could not activate host prefix");

    let build_activation_script = if let Some(build_environment) = build_environment {
        // We use the previous PATH and _no_ CONDA_PREFIX to stack the build
        // prefix on top of the host prefix
        let activation_vars = ActivationVariables {
            conda_prefix: None,
            path: Some(host_activation.path.clone()),
            path_modification_behavior: Default::default(),
        };

        let activator =
            Activator::from_path(&build_environment, shell.clone(), Platform::current())?;
        activator.activation(activation_vars)?.script
    } else {
        String::new()
    };

    let mut script_content = String::new();
    let mut additional_script = ShellScript::new(shell.clone(), Platform::current());

    let os_vars = env_vars::os_vars(environment, &Platform::current());
    for (key, val) in os_vars {
        if key == "PATH" {
            continue;
        }
        additional_script.set_env_var(&key, &val);
    }

    additional_script.set_env_var("PREFIX", environment.to_string_lossy().as_ref());
    writeln!(script_content, "{}", additional_script.contents)?;
    writeln!(script_content, "{}", host_activation.script)?;
    writeln!(script_content, "{}", build_activation_script)?;
    if matches!(shell, ShellEnum::Bash(_)) {
        writeln!(script_content, "set -x")?;
    }
    writeln!(script_content, "{}", cmd)?;

    let mut tmpfile = tempfile::Builder::new()
        .prefix("rattler-test-")
        .suffix(&format!(".{}", shell.extension()))
        .tempfile()?;

    if matches!(shell, ShellEnum::CmdExe(_)) {
        script_content = format!("chcp 65001 > nul\n{script_content}").replace('\n', "\r\n");
        tmpfile.write_all(script_content.as_bytes())?;
    } else {
        tmpfile.write_all(script_content.as_bytes())?;
    }

    let tmpfile_path = tmpfile.into_temp_path();

    tracing::info!("Running test script:\n{}", script_content);

    let executable = shell.executable();
    let status = match shell {
        ShellEnum::Bash(_) => std::process::Command::new(executable)
            .arg("-e")
            .arg(&tmpfile_path)
            .current_dir(cwd)
            .status()?,
        ShellEnum::CmdExe(_) => std::process::Command::new(executable)
            .arg("/d")
            .arg("/c")
            .arg(&tmpfile_path)
            .current_dir(cwd)
            .status()?,
        _ => todo!("No shells implemented beyond cmd.exe and bash"),
    };

    if !status.success() {
        return Err(TestError::TestFailed);
    }

    Ok(())
}

impl Tests {
    fn run(&self, environment: &Path, cwd: &Path) -> Result<(), TestError> {
        let default_shell = ShellEnum::default();

        match self {
            Tests::Commands(path) => {
                let contents = fs::read_to_string(path)?;
                let is_path_ext =
                    |ext: &str| path.extension().map(|s| s.eq(ext)).unwrap_or_default();
                if Platform::current().is_windows() && is_path_ext("bat") {
                    tracing::info!("Testing commands:");
                    run_in_environment(default_shell, contents, cwd, environment, None)
                } else if Platform::current().is_unix() && is_path_ext("sh") {
                    tracing::info!("Testing commands:");
                    run_in_environment(default_shell, contents, cwd, environment, None)
                } else {
                    Ok(())
                }
            }
            Tests::Python(path) => {
                let imports = fs::read_to_string(path)?;
                tracing::info!("Testing Python imports:\n{imports}");
                run_in_environment(
                    default_shell,
                    format!("python {}", path.to_string_lossy()),
                    cwd,
                    environment,
                    None,
                )
            }
        }
    }
}

async fn legacy_tests_from_folder(pkg: &Path) -> Result<(PathBuf, Vec<Tests>), TestError> {
    let mut tests = Vec::new();

    let test_folder = pkg.join("info/test");

    if !test_folder.exists() {
        return Ok((test_folder, tests));
    }

    let mut read_dir = tokio::fs::read_dir(&test_folder).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        if file_name.eq("run_test.sh") || file_name.eq("run_test.bat") {
            println!("test {}", file_name.to_string_lossy());
            tests.push(Tests::Commands(path));
        } else if file_name.eq("run_test.py") {
            println!("test {}", file_name.to_string_lossy());
            tests.push(Tests::Python(path));
        }
    }

    Ok((test_folder, tests))
}

/// The configuration for a test
#[derive(Default, Clone, Debug)]
pub struct TestConfiguration {
    /// The test prefix directory (will be created)
    pub test_prefix: PathBuf,
    /// The target platform
    pub target_platform: Option<Platform>,
    /// If true, the test prefix will not be deleted after the test is run
    pub keep_test_prefix: bool,
    /// The channels to use for the test – do not forget to add the local build outputs channel
    /// if desired
    pub channels: Vec<String>,
    /// The tool configuration
    pub tool_configuration: tool_configuration::Configuration,
}

/// Run a test for a single package
///
/// This function creates a temporary directory, copies the package file into it, and then runs the
/// indexing. It then creates a test environment that installs the package and any extra dependencies
/// specified in the package test dependencies file.
///
/// With the activated test environment, the packaged test files are run:
///
/// * `info/test/run_test.sh` or `info/test/run_test.bat` on Windows
/// * `info/test/run_test.py`
///
/// These test files are written at "package creation time" and are part of the package.
///
/// # Arguments
///
/// * `package_file` - The path to the package file
/// * `config` - The test configuration
///
/// # Returns
///
/// * `Ok(())` if the test was successful
/// * `Err(TestError::TestFailed)` if the test failed
pub async fn run_test(package_file: &Path, config: &TestConfiguration) -> Result<(), TestError> {
    let tmp_repo = tempfile::tempdir()?;

    let target_platform = if let Some(tp) = config.target_platform {
        tp
    } else {
        let index_json: IndexJson =
            rattler_package_streaming::seek::read_package_file(package_file)
                .map_err(|_| TestError::CouldNotDetermineTargetPlatform)?;
        let subdir = index_json
            .subdir
            .ok_or(TestError::CouldNotDetermineTargetPlatform)?;
        Platform::from_str(&subdir).map_err(|_| TestError::CouldNotDetermineTargetPlatform)?
    };

    let subdir = tmp_repo.path().join(target_platform.to_string());
    std::fs::create_dir_all(&subdir)?;

    std::fs::copy(
        package_file,
        subdir.join(
            package_file
                .file_name()
                .ok_or(TestError::MissingPackageFileName)?,
        ),
    )?;

    // index the temporary channel
    index(tmp_repo.path(), Some(&target_platform))?;

    let cache_dir = rattler::default_cache_dir()?;

    let pkg = ArchiveIdentifier::try_from_path(package_file).ok_or(TestError::TestFailed)?;

    // if the package is already in the cache, remove it. TODO make this based on SHA256 instead!
    let cache_key = CacheKey::from(pkg.clone());
    let package_folder = cache_dir.join("pkgs").join(cache_key.to_string());

    if package_folder.exists() {
        tracing::info!("Removing previously cached package {:?}", &package_folder);
        fs::remove_dir_all(&package_folder)?;
    }

    let prefix = canonicalize(&config.test_prefix)?;

    tracing::info!("Creating test environment in {:?}", prefix);

    let platform = if target_platform != Platform::NoArch {
        target_platform
    } else {
        Platform::current()
    };

    let mut channels = config.channels.clone();
    channels.insert(0, tmp_repo.path().to_string_lossy().to_string());

    let config = TestConfiguration {
        target_platform: Some(target_platform),
        channels,
        ..config.clone()
    };

    tracing::info!("Collecting tests from {:?}", package_folder);

    rattler_package_streaming::fs::extract(package_file, &package_folder).map_err(|e| {
        tracing::error!("Failed to extract package: {:?}", e);
        TestError::TestFailed
    })?;

    // extract package in place
    if package_folder.join("info/test").exists() {
        let test_dep_json = PathBuf::from("info/test/test_time_dependencies.json");
        let test_dependencies: Vec<String> = if package_folder.join(&test_dep_json).exists() {
            serde_json::from_str(&std::fs::read_to_string(
                package_folder.join(&test_dep_json),
            )?)?
        } else {
            Vec::new()
        };

        let mut dependencies: Vec<MatchSpec> = test_dependencies
            .iter()
            .map(|s| MatchSpec::from_str(s))
            .collect::<Result<Vec<_>, _>>()?;

        tracing::info!("Creating test environment in {:?}", prefix);
        let match_spec = MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
        )
        .map_err(|e| TestError::MatchSpecParse(e.to_string()))?;
        dependencies.push(match_spec);

        create_environment(
            &dependencies,
            &platform,
            &prefix,
            &config.channels,
            &config.tool_configuration,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        // These are the legacy tests
        let (test_folder, tests) = legacy_tests_from_folder(&package_folder).await?;

        for test in tests {
            test.run(&prefix, &test_folder)?;
        }

        tracing::info!(
            "{} all tests passed!",
            console::style(console::Emoji("✔", "")).green()
        );
    }

    if package_folder.join("info/tests").exists() {
        // These are the new style tests
        let test_folder = package_folder.join("info/tests");
        let mut read_dir = tokio::fs::read_dir(&test_folder).await?;

        // for each enumerated test, we load and run it
        while let Some(entry) = read_dir.next_entry().await? {
            println!("test {:?}", entry.path());
            run_individual_test(&pkg, &entry.path(), &prefix, &config).await?;
        }

        tracing::info!(
            "{} all tests passed!",
            console::style(console::Emoji("✔", "")).green()
        );
    }

    fs::remove_dir_all(prefix)?;

    Ok(())
}

async fn run_python_test(
    pkg: &ArchiveIdentifier,
    path: &Path,
    prefix: &Path,
    config: &TestConfiguration,
) -> Result<(), TestError> {
    let test_file = path.join("python_test.json");
    let test: PythonTest = serde_json::from_str(&fs::read_to_string(test_file)?)?;

    let match_spec =
        MatchSpec::from_str(format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str())
            .unwrap();
    let mut dependencies = vec![match_spec];
    if test.pip_check {
        dependencies.push(MatchSpec::from_str("pip").unwrap());
    }

    let platform = Platform::current();

    create_environment(
        &dependencies,
        &platform,
        prefix,
        &config.channels,
        &config.tool_configuration,
    )
    .await
    .map_err(TestError::TestEnvironmentSetup)?;

    let default_shell = ShellEnum::default();

    let mut test_file = tempfile::Builder::new()
        .prefix("rattler-test-")
        .suffix(".py")
        .tempfile()?;

    for import in test.imports {
        writeln!(test_file, "import {}", import)?;
    }

    run_in_environment(
        default_shell.clone(),
        format!("python {}", test_file.path().to_string_lossy()),
        path,
        prefix,
        None,
    )?;

    if test.pip_check {
        run_in_environment(default_shell, "pip check".into(), path, prefix, None)
    } else {
        Ok(())
    }
}

async fn run_shell_test(
    pkg: &ArchiveIdentifier,
    path: &Path,
    prefix: &Path,
    config: &TestConfiguration,
) -> Result<(), TestError> {
    let deps = if path.join("test_time_dependencies.json").exists() {
        let test_dep_json = path.join("test_time_dependencies.json");
        serde_json::from_str(&fs::read_to_string(test_dep_json)?)?
    } else {
        CommandsTestRequirements::default()
    };

    let build_env = if !deps.build.is_empty() {
        tracing::info!("Installing build dependencies");
        let build_prefix = prefix.join("bld");
        let platform = Platform::current();
        let build_dependencies = deps
            .build
            .iter()
            .map(|s| MatchSpec::from_str(s))
            .collect::<Result<Vec<_>, _>>()?;

        create_environment(
            &build_dependencies,
            &platform,
            &build_prefix,
            &config.channels,
            &config.tool_configuration,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;
        Some(build_prefix)
    } else {
        None
    };

    let mut dependencies = deps
        .run
        .iter()
        .map(|s| MatchSpec::from_str(s))
        .collect::<Result<Vec<_>, _>>()?;

    // create environment with the test dependencies
    dependencies.push(MatchSpec::from_str(
        format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
    )?);

    let platform = config.target_platform.unwrap_or_else(Platform::current);

    let run_env = prefix.join("run");
    create_environment(
        &dependencies,
        &platform,
        &run_env,
        &config.channels,
        &config.tool_configuration,
    )
    .await
    .map_err(TestError::TestEnvironmentSetup)?;

    let default_shell = ShellEnum::default();

    let test_file_path = if platform.is_windows() {
        path.join("run_test.bat")
    } else {
        path.join("run_test.sh")
    };

    let contents = fs::read_to_string(test_file_path)?;

    tracing::info!("Testing commands:");
    run_in_environment(default_shell, contents, path, &run_env, build_env)?;

    Ok(())
}

async fn run_individual_test(
    pkg: &ArchiveIdentifier,
    path: &Path,
    prefix: &Path,
    config: &TestConfiguration,
) -> Result<(), TestError> {
    if path.join("python_test.json").exists() {
        run_python_test(pkg, path, prefix, config).await?;
    } else if path.join("run_test.sh").exists() || path.join("run_test.bat").exists() {
        // run shell test
        run_shell_test(pkg, path, prefix, config).await?;
    } else {
        // no test found
    }

    println!(
        "{} test passed!",
        console::style(console::Emoji("✔", "")).green()
    );

    Ok(())
}
