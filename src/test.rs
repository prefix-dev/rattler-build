//! Testing a package produced by rattler-build (or conda-build)
//!
//! Tests are part of the final package (under the `info/test` directory).
//! There are multiple test types:
//!
//! * `commands` - run a list of commands and check their exit code
//! * `imports` - import a list of modules and check if they can be imported
//! * `files` - check if a list of files exist

use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    str::FromStr,
};

use rattler::package_cache::CacheKey;
use rattler_conda_types::{
    package::{ArchiveIdentifier, ArchiveType},
    MatchSpec, Platform,
};
use rattler_shell::activation::ActivationVariables;
use rattler_shell::{activation::Activator, shell};
use std::io::Write;

use crate::{env_vars, index, render::solver::create_environment};

#[derive(thiserror::Error, Debug)]
pub enum TestError {
    #[error("failed to run test")]
    TestFailed,

    #[error("Failed to read package: {0}")]
    PackageRead(#[from] std::io::Error),

    #[error("Failed to parse MatchSpec: {0}")]
    MatchSpecParse(String),

    #[error("Failed to setup test environment:")]
    TestEnvironmentSetup,
}

#[derive(Debug)]
enum Tests {
    Commands(PathBuf),
    Python(PathBuf),
}

fn run_in_environment(cmd: &str, environment: &Path) -> Result<(), TestError> {
    let current_path = std::env::var("PATH")
        .ok()
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>());

    // if we are in a conda environment, we need to deactivate it before activating the host / build prefix
    let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

    let av = ActivationVariables {
        conda_prefix,
        path: current_path,
    };

    let activator = Activator::from_path(environment, shell::Bash, Platform::current()).unwrap();
    let script = activator.activation(av).unwrap();

    let tmpfile = tempfile::NamedTempFile::new().unwrap();
    let tmpfile_path = tmpfile.path();

    let mut out_script = File::create(tmpfile_path).unwrap();
    let os_vars = env_vars::os_vars(environment, &Platform::current());

    for var in os_vars {
        writeln!(out_script, "export {}=\"{}\"", var.0, var.1)?;
    }

    writeln!(out_script, "{}", script.script)?;
    writeln!(
        out_script,
        "export PREFIX={}",
        environment.to_string_lossy()
    )?;
    writeln!(out_script, "{}", cmd)?;

    let mut cmd = std::process::Command::new("bash");
    cmd.arg(tmpfile_path);

    let status = cmd.status().unwrap();

    if !status.success() {
        return Err(TestError::TestFailed);
    }

    Ok(())
}

impl Tests {
    fn run(&self, environment: &Path) -> Result<(), TestError> {
        match self {
            Tests::Commands(path) => {
                let ext = path.extension().unwrap().to_str().unwrap();
                match (Platform::current().is_windows(), ext) {
                    (true, "bat") => {
                        tracing::info!("Testing commands:");
                        run_in_environment(
                            &format!("cmd /c {}", path.to_string_lossy()),
                            environment,
                        )
                    }
                    (false, "sh") => {
                        tracing::info!("Testing commands:");
                        run_in_environment(
                            &format!("bash -x {}", path.to_string_lossy()),
                            environment,
                        )
                    }
                    _ => Ok(()),
                }
            }
            Tests::Python(path) => {
                let imports = fs::read_to_string(path)?;
                tracing::info!("Testing Python imports:\n{imports}");
                run_in_environment(&format!("python {}", path.to_string_lossy()), environment)
            }
        }
    }
}

async fn tests_from_folder(pkg: &Path) -> Result<Vec<Tests>, TestError> {
    let mut tests = Vec::new();

    let test_folder = pkg.join("info").join("test");

    if !test_folder.exists() {
        return Ok(tests);
    }

    let mut read_dir = tokio::fs::read_dir(test_folder).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let file_name = path.file_name().unwrap().to_str().unwrap();
        match file_name {
            "run_test.sh" | "run_test.bat" => tests.push(Tests::Commands(path)),
            "run_test.py" => tests.push(Tests::Python(path)),
            _ => {}
        }
    }

    Ok(tests)
}

fn file_from_tar_bz2(archive_path: &Path, find_path: &Path) -> Result<String, std::io::Error> {
    let reader = std::fs::File::open(archive_path).unwrap();
    let mut archive = rattler_package_streaming::read::stream_tar_bz2(reader);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path == find_path {
            let mut contents = String::new();
            entry.read_to_string(&mut contents)?;
            return Ok(contents);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{:?} not found in {:?}", find_path, archive_path),
    ))
}

fn file_from_conda(archive_path: &Path, find_path: &Path) -> Result<String, std::io::Error> {
    let reader = std::fs::File::open(archive_path).unwrap();

    let mut archive = if find_path.starts_with("info") {
        rattler_package_streaming::seek::stream_conda_info(reader)
            .expect("Could not open conda file")
    } else {
        todo!("Not implemented yet");
    };

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path == find_path {
            let mut contents = String::new();
            entry.read_to_string(&mut contents)?;
            return Ok(contents);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{:?} not found in {:?}", find_path, archive_path),
    ))
}

#[derive(Default, Debug)]
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
    let tmp_repo = tempfile::tempdir().unwrap();
    let target_platform = config.target_platform.unwrap_or_else(Platform::current);

    let subdir = tmp_repo.path().join(target_platform.to_string());
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::copy(package_file, subdir.join(package_file.file_name().unwrap())).unwrap();

    let archive_type = ArchiveType::try_from(package_file).unwrap();
    let test_dep_json = PathBuf::from("info/test/test_time_dependencies.json");
    let test_dependencies = match archive_type {
        ArchiveType::TarBz2 => file_from_tar_bz2(package_file, &test_dep_json),
        ArchiveType::Conda => file_from_conda(package_file, &test_dep_json),
    };

    let mut dependencies: Vec<MatchSpec> = match test_dependencies {
        Ok(contents) => {
            let test_deps: Vec<String> = serde_json::from_str(&contents).unwrap();
            test_deps
                .iter()
                .map(|s| MatchSpec::from_str(s).unwrap())
                .collect()
        }
        Err(error) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                Vec::new()
            } else {
                return Err(TestError::TestFailed);
            }
        }
    };

    // index the temporary channel
    index::index(tmp_repo.path(), Some(&target_platform)).unwrap();

    let cache_dir = rattler::default_cache_dir().unwrap();

    let pkg = ArchiveIdentifier::try_from_path(package_file).ok_or(TestError::TestFailed)?;
    let match_spec =
        MatchSpec::from_str(format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str())
            .map_err(|e| TestError::MatchSpecParse(e.to_string()))?;
    dependencies.push(match_spec);

    let prefix = std::fs::canonicalize(&config.test_prefix).unwrap();
    create_environment(
        &dependencies,
        &Platform::current(),
        &prefix,
        &config.channels,
    )
    .await
    .map_err(|_| TestError::TestEnvironmentSetup)?;

    let cache_key = CacheKey::from(pkg);
    let dir = cache_dir.join("pkgs").join(cache_key.to_string());

    println!("Collecting tests from {:?}", dir);
    let tests = tests_from_folder(&dir).await?;

    for test in tests {
        test.run(&prefix)?;
    }

    println!("Tests passed!");

    fs::remove_dir_all(prefix).unwrap();

    Ok(())
}
