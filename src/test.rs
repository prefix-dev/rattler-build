//! Testing a package produced by rattler-build (or conda-build)
//!
//! Tests are part of the final package (under the `info/test` directory).
//! There are multiple test types:
//!
//! * `commands` - run a list of commands and check their exit code
//! * `imports` - import a list of modules and check if they can be imported
//! * `files` - check if a list of files exist

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use rattler::package_cache::CacheKey;
use rattler_conda_types::{package::ArchiveIdentifier, MatchSpec, NoArchType, Platform};

use crate::{index, metadata::PlatformOrNoarch, render::solver::create_environment};

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

pub fn setup_test_environment() -> Result<(), TestError> {
    Ok(())
}

#[derive(Debug)]
enum Tests {
    Commands(PathBuf),
    Python(PathBuf),
}

use rattler_shell::activation::ActivationVariables;
use rattler_shell::{activation::Activator, shell};

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

    let final_script = format!("{}\n{}", script.script, cmd);

    std::fs::write(tmpfile_path, final_script).unwrap();

    let mut cmd = std::process::Command::new("bash");
    cmd.arg(tmpfile_path);

    let status = cmd.status().unwrap();

    if !status.success() {
        return Err(TestError::TestFailed);
    }

    Ok(())
}

impl Tests {}

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
            _ => {
                tracing::warn!("Unknown test file: {}", file_name)
            }
        }
    }

    Ok(tests)
}

pub async fn run_test(
    package_file: &Path,
    test_prefix: Option<&Path>,
    target_platform: Option<Platform>,
) -> Result<(), TestError> {
    let tmp_repo = tempfile::tempdir().unwrap();
    let target_platform = target_platform.unwrap_or_else(Platform::current);

    let target_platform = match target_platform {
        Platform::NoArch => PlatformOrNoarch::Noarch(NoArchType(None)),
        p => PlatformOrNoarch::Platform(p),
    };

    let subdir = tmp_repo.path().join(target_platform.to_string());
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::copy(package_file, subdir.join(package_file.file_name().unwrap())).unwrap();

    // index the temporary channel
    index::index(tmp_repo.path(), Some(&target_platform)).unwrap();

    let cache_dir = rattler::default_cache_dir().unwrap();

    let pkg = ArchiveIdentifier::try_from_path(package_file).ok_or(TestError::TestFailed)?;
    let match_spec =
        MatchSpec::from_str(format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str())
            .map_err(|e| TestError::MatchSpecParse(e.to_string()))?;

    let build_output_folder = std::fs::canonicalize(Path::new("./output")).unwrap();

    let channels = vec![
        build_output_folder.to_string_lossy().to_string(),
        "conda-forge".to_string(),
    ];
    let prefix = test_prefix.unwrap_or_else(|| Path::new("./test-env"));
    create_environment(vec![match_spec], &Platform::current(), prefix, &channels)
        .await
        .map_err(|_| TestError::TestEnvironmentSetup)?;

    let cache_key = CacheKey::from(pkg);
    let dir = cache_dir.join("pkgs").join(cache_key.to_string());

    let tests = tests_from_folder(&dir).await?;
    println!("Running tests: {:?}", tests);

    // for test in tests {}

    Ok(())
}
