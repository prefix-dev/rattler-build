//! Testing a package produced by rattler-build (or conda-build)
//!
//! Tests are part of the final package (under the `info/test` directory).
//! There are multiple test types:
//!
//! * `commands` - run a list of commands and check their exit code
//! * `imports` - import a list of modules and check if they can be imported
//! * `files` - check if a list of files exist

use std::{
    collections::HashMap,
    fmt::Write as fmt_write,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use fs_err as fs;
use rattler::package_cache::CacheKey;
use rattler_conda_types::{
    Channel, ChannelUrl, MatchSpec, ParseStrictness, Platform,
    package::{ArchiveIdentifier, IndexJson, PackageFile},
};
use rattler_index::{IndexFsConfig, index_fs};
use rattler_shell::{
    activation::ActivationError,
    shell::{Shell, ShellEnum},
};
use rattler_solve::{ChannelPriority, SolveStrategy};
use tempfile::TempDir;

use crate::{
    env_vars,
    metadata::{Debug, PlatformWithVirtualPackages},
    recipe::parser::{
        CommandsTest, DownstreamTest, PerlTest, PythonTest, PythonVersion, RTest, Script,
        ScriptContent, TestType,
    },
    render::solver::create_environment,
    source::copy_dir::CopyDir,
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

    #[error("failed to run test: {0}")]
    TestFailed(String),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("failed to write testing script: {0}")]
    FailedToWriteScript(#[from] std::fmt::Error),

    #[error("failed to parse MatchSpec: {0}")]
    MatchSpecParse(String),

    #[error("failed to setup test environment: {0}")]
    TestEnvironmentSetup(#[from] anyhow::Error),

    #[error("failed to setup test environment: {0}")]
    TestEnvironmentActivation(#[from] ActivationError),

    #[error("failed to parse tests from `info/tests/tests.yaml`: {0}")]
    TestYamlParseError(#[from] serde_yaml::Error),

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

impl Tests {
    async fn run(
        &self,
        environment: &Path,
        cwd: &Path,
        pkg_vars: &HashMap<String, String>,
    ) -> Result<(), TestError> {
        tracing::info!("Testing commands:");

        let platform = Platform::current();
        let mut env_vars = env_vars::os_vars(environment, &platform);
        env_vars.retain(|key, _| key != ShellEnum::default().path_var(&platform));
        env_vars.extend(pkg_vars.iter().map(|(k, v)| (k.clone(), Some(v.clone()))));
        env_vars.insert(
            "PREFIX".to_string(),
            Some(environment.to_string_lossy().to_string()),
        );
        let tmp_dir = tempfile::tempdir()?;

        match self {
            Tests::Commands(path) => {
                let script = Script {
                    content: ScriptContent::Path(path.clone()),
                    ..Script::default()
                };

                // copy all test files to a temporary directory and set it as the working
                // directory
                CopyDir::new(path, tmp_dir.path()).run().map_err(|e| {
                    TestError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to copy test files: {}", e),
                    ))
                })?;

                script
                    .run_script(
                        env_vars,
                        tmp_dir.path(),
                        cwd,
                        environment,
                        None,
                        None,
                        None,
                        Debug::new(false),
                    )
                    .await
                    .map_err(|e| TestError::TestFailed(e.to_string()))?;
            }
            Tests::Python(path) => {
                let script = Script {
                    content: ScriptContent::Path(path.clone()),
                    interpreter: Some("python".into()),
                    ..Script::default()
                };

                script
                    .run_script(
                        env_vars,
                        tmp_dir.path(),
                        cwd,
                        environment,
                        None,
                        None,
                        None,
                        Debug::new(false),
                    )
                    .await
                    .map_err(|e| TestError::TestFailed(e.to_string()))?;
            }
        }
        Ok(())
    }
}

async fn legacy_tests_from_folder(pkg: &Path) -> Result<(PathBuf, Vec<Tests>), std::io::Error> {
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
            tracing::info!("test {}", file_name.to_string_lossy());
            tests.push(Tests::Commands(path));
        } else if file_name.eq("run_test.py") {
            tracing::info!("test {}", file_name.to_string_lossy());
            tests.push(Tests::Python(path));
        }
    }

    Ok((test_folder, tests))
}

/// The configuration for a test
#[derive(Clone)]
pub struct TestConfiguration {
    /// The test prefix directory (will be created)
    pub test_prefix: PathBuf,
    /// The target platform. If not set it will be discovered from the
    /// index.json metadata.
    pub target_platform: Option<Platform>,
    /// The host platform for run-time dependencies. If not set it will be
    /// discovered from the index.json metadata.
    pub host_platform: Option<PlatformWithVirtualPackages>,
    /// The platform and virtual packages of the current platform.
    pub current_platform: PlatformWithVirtualPackages,
    /// If true, the test prefix will not be deleted after the test is run
    pub keep_test_prefix: bool,
    /// The index of the test to execute. If not set, all tests will be executed.
    pub test_index: Option<usize>,
    /// The channels to use for the test – do not forget to add the local build
    /// outputs channel if desired
    pub channels: Vec<ChannelUrl>,
    /// The channel priority that is used to resolve dependencies
    pub channel_priority: ChannelPriority,
    /// The solve strategy to use when resolving dependencies
    pub solve_strategy: SolveStrategy,
    /// The tool configuration
    pub tool_configuration: tool_configuration::Configuration,
    /// The output directory to create the test prefixes in (will be `output_dir/test`)
    pub output_dir: PathBuf,
    /// Debug mode yes, or no
    pub debug: Debug,
}

fn env_vars_from_package(index_json: &IndexJson) -> HashMap<String, String> {
    let mut res = HashMap::new();

    res.insert(
        "PKG_NAME".to_string(),
        index_json.name.as_normalized().to_string(),
    );
    res.insert("PKG_VERSION".to_string(), index_json.version.to_string());
    res.insert("PKG_BUILD_STRING".to_string(), index_json.build.clone());
    res.insert(
        "PKG_BUILDNUM".to_string(),
        index_json.build_number.to_string(),
    );
    res.insert(
        "PKG_BUILD_NUMBER".to_string(),
        index_json.build_number.to_string(),
    );

    res
}

/// Run a test for a single package
///
/// This function creates a temporary directory, copies the package file into
/// it, and then runs the indexing. It then creates a test environment that
/// installs the package and any extra dependencies specified in the package
/// test dependencies file.
///
/// With the activated test environment, the packaged test files are run:
///
/// * `info/test/run_test.sh` or `info/test/run_test.bat` on Windows
/// * `info/test/run_test.py`
///
/// These test files are written at "package creation time" and are part of the
/// package.
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
#[async_recursion::async_recursion]
pub async fn run_test(
    package_file: &Path,
    config: &TestConfiguration,
    downstream_package: Option<PathBuf>,
) -> Result<(), TestError> {
    let tmp_repo = tempfile::tempdir()?;

    // create the test prefix
    fs::create_dir_all(&config.test_prefix)?;

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
    fs::create_dir_all(&subdir)?;

    fs::copy(
        package_file,
        subdir.join(
            package_file
                .file_name()
                .ok_or(TestError::MissingPackageFileName)?,
        ),
    )?;

    // Also copy the downstream package if it exists
    if let Some(ref downstream_package) = downstream_package {
        fs::copy(
            downstream_package,
            subdir.join(
                downstream_package
                    .file_name()
                    .ok_or(TestError::MissingPackageFileName)?,
            ),
        )?;
    }

    // if there is a downstream package, that's the one we actually want to test
    let package_file = downstream_package.as_deref().unwrap_or(package_file);

    let index_config = IndexFsConfig {
        channel: tmp_repo.path().to_path_buf(),
        target_platform: Some(target_platform),
        repodata_patch: None,
        write_zst: false,
        write_shards: false,
        force: false,
        max_parallel: num_cpus::get_physical(),
        multi_progress: None,
    };

    // index the temporary channel
    index_fs(index_config).await?;

    let cache_dir = rattler::default_cache_dir()?;

    let pkg = ArchiveIdentifier::try_from_path(package_file)
        .ok_or_else(|| TestError::TestFailed("could not get archive identifier".to_string()))?;

    // if the package is already in the cache, remove it.
    // TODO make this based on SHA256 instead!
    let cache_key = CacheKey::from(pkg.clone());
    let package_folder = cache_dir.join("pkgs").join(cache_key.to_string());

    if package_folder.exists() {
        tracing::info!(
            "Removing previously cached package '{}'",
            package_folder.display()
        );
        fs::remove_dir_all(&package_folder)?;
    }

    let mut channels = config.channels.clone();
    channels.insert(0, Channel::from_directory(tmp_repo.path()).base_url);

    let host_platform = config.host_platform.clone().unwrap_or_else(|| {
        if target_platform == Platform::NoArch {
            config.current_platform.clone()
        } else {
            PlatformWithVirtualPackages {
                platform: target_platform,
                virtual_packages: config.current_platform.virtual_packages.clone(),
            }
        }
    });

    let config = TestConfiguration {
        target_platform: Some(target_platform),
        host_platform: Some(host_platform.clone()),
        channels,
        ..config.clone()
    };

    tracing::info!("Collecting tests from '{}'", package_folder.display());

    rattler_package_streaming::fs::extract(package_file, &package_folder).map_err(|e| {
        tracing::error!("Failed to extract package: {:?}", e);
        TestError::TestFailed(format!("failed to extract package: {:?}", e))
    })?;

    let index_json = IndexJson::from_package_directory(&package_folder)?;
    let env = env_vars_from_package(&index_json);
    // extract package in place
    if package_folder.join("info/test").exists() {
        let prefix =
            TempDir::with_prefix_in(format!("test_{}", pkg.name), &config.output_dir)?.keep();

        tracing::info!("Creating test environment in '{}'", prefix.display());

        let test_dep_json = PathBuf::from("info/test/test_time_dependencies.json");
        let test_dependencies: Vec<String> = if package_folder.join(&test_dep_json).exists() {
            serde_json::from_str(&fs::read_to_string(package_folder.join(&test_dep_json))?)?
        } else {
            Vec::new()
        };

        let mut dependencies: Vec<MatchSpec> = test_dependencies
            .iter()
            .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
            .collect::<Result<Vec<_>, _>>()?;

        tracing::info!("Creating test environment in {:?}", prefix);
        let match_spec = MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
            ParseStrictness::Lenient,
        )
        .map_err(|e| TestError::MatchSpecParse(e.to_string()))?;
        dependencies.push(match_spec);

        create_environment(
            "test",
            &dependencies,
            &host_platform,
            &prefix,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        // These are the legacy tests
        let (test_folder, tests) = legacy_tests_from_folder(&package_folder).await?;

        for test in tests {
            test.run(&prefix, &test_folder, &env).await?;
        }

        tracing::info!(
            "{} all tests passed!",
            console::style(console::Emoji("✔", "")).green()
        );

        if prefix.exists() {
            fs::remove_dir_all(prefix)?;
        }
    }

    if package_folder.join("info/tests/tests.yaml").exists() {
        let tests = fs::read_to_string(package_folder.join("info/tests/tests.yaml"))?;
        let tests: Vec<TestType> = serde_yaml::from_str(&tests)?;

        if let Some(test_index) = config.test_index {
            if test_index >= tests.len() {
                return Err(TestError::TestFailed(format!(
                    "Test index {} out of range (0..{})",
                    test_index,
                    tests.len()
                )));
            }
        }

        let tests = if let Some(test_index) = config.test_index {
            vec![tests[test_index].clone()]
        } else {
            tests
        };

        for test in tests {
            let test_prefix =
                TempDir::with_prefix_in(format!("test_{}", pkg.name), &config.test_prefix)?.keep();
            match test {
                TestType::Command(c) => {
                    c.run_test(&pkg, &package_folder, &test_prefix, &config, &env)
                        .await?
                }
                TestType::Python { python } => {
                    python
                        .run_test(&pkg, &package_folder, &test_prefix, &config)
                        .await?
                }
                TestType::Perl { perl } => {
                    perl.run_test(&pkg, &package_folder, &test_prefix, &config)
                        .await?
                }
                TestType::R { r } => {
                    r.run_test(&pkg, &package_folder, &test_prefix, &config)
                        .await?
                }
                TestType::Downstream(downstream) if downstream_package.is_none() => {
                    downstream
                        .run_test(&pkg, package_file, &test_prefix, &config)
                        .await?
                }
                TestType::Downstream(_) => {
                    tracing::info!(
                        "Skipping downstream test as we are already testing a downstream package"
                    )
                }
                // This test already runs during the build process and we don't need to run it again
                TestType::PackageContents { .. } => {}
            }

            if !config.keep_test_prefix {
                fs::remove_dir_all(test_prefix)?;
            }
        }

        tracing::info!(
            "{} all tests passed!",
            console::style(console::Emoji("✔", "")).green()
        );
    }

    Ok(())
}

impl PythonTest {
    /// Execute the Python test
    pub async fn run_test(
        &self,
        pkg: &ArchiveIdentifier,
        path: &Path,
        prefix: &Path,
        config: &TestConfiguration,
    ) -> Result<(), TestError> {
        let span = tracing::info_span!("Running python test(s)");
        let _guard = span.enter();

        // The version spec of the package being built
        let match_spec = MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
            ParseStrictness::Lenient,
        )?;

        // The dependencies for the test environment
        // - python_version: null -> { "": ["mypackage=xx=xx"]}
        // - python_version: 3.12 -> { "3.12": ["python=3.12", "mypackage=xx=xx"]}
        // - python_version: [3.12, 3.13] -> { "3.12": ["python=3.12", "mypackage=xx=xx"], "3.13": ["python=3.13", "mypackage=xx=xx"]}
        let mut dependencies_map: HashMap<String, Vec<MatchSpec>> = match &self.python_version {
            PythonVersion::Multiple(versions) => versions
                .iter()
                .map(|version| {
                    (
                        version.clone(),
                        vec![
                            MatchSpec::from_str(
                                &format!("python={}", version),
                                ParseStrictness::Lenient,
                            )
                            .unwrap(),
                            match_spec.clone(),
                        ],
                    )
                })
                .collect(),
            PythonVersion::Single(version) => HashMap::from([(
                version.clone(),
                vec![
                    MatchSpec::from_str(&format!("python={}", version), ParseStrictness::Lenient)
                        .unwrap(),
                    match_spec,
                ],
            )]),
            PythonVersion::None => HashMap::from([("".to_string(), vec![match_spec])]),
        };

        // Add `pip` if pip_check is set to true
        if self.pip_check {
            dependencies_map
                .iter_mut()
                .for_each(|(_, v)| v.push("pip".parse().unwrap()));
        }

        // Run tests for each python version
        for (python_version, dependencies) in dependencies_map {
            self.run_test_inner(python_version, dependencies, path, prefix, config)
                .await?;
        }

        Ok(())
    }

    async fn run_test_inner(
        &self,
        python_version: String,
        dependencies: Vec<MatchSpec>,
        path: &Path,
        prefix: &Path,
        config: &TestConfiguration,
    ) -> Result<(), TestError> {
        let span_message = match python_version.as_str() {
            "" => "Testing with default python version".to_string(),
            _ => format!("Testing with python {}", python_version),
        };
        let span = tracing::info_span!("", message = %span_message);
        let _guard = span.enter();

        let test_prefix = prefix.join("test_env");
        create_environment(
            "test",
            &dependencies,
            config
                .host_platform
                .as_ref()
                .unwrap_or(&config.current_platform),
            &test_prefix,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        let mut imports = String::new();
        for import in &self.imports {
            writeln!(imports, "import {}", import)?;
        }

        let script = Script {
            content: ScriptContent::Command(imports),
            interpreter: Some("python".into()),
            ..Script::default()
        };

        let test_dir = prefix.join("test");
        fs::create_dir_all(&test_dir)?;
        script
            .run_script(
                Default::default(),
                &test_dir,
                path,
                &test_prefix,
                None,
                None,
                None,
                config.debug,
            )
            .await
            .map_err(|e| TestError::TestFailed(e.to_string()))?;

        tracing::info!(
            "{} python imports test passed!",
            console::style(console::Emoji("✔", "")).green()
        );

        if self.pip_check {
            let script = Script {
                content: ScriptContent::Command("pip check".into()),
                ..Script::default()
            };
            script
                .run_script(
                    Default::default(),
                    path,
                    path,
                    &test_prefix,
                    None,
                    None,
                    None,
                    config.debug,
                )
                .await
                .map_err(|e| TestError::TestFailed(e.to_string()))?;

            tracing::info!(
                "{} pip check passed!",
                console::style(console::Emoji("✔", "")).green()
            );
        }
        Ok(())
    }
}

impl PerlTest {
    /// Execute the Perl test
    pub async fn run_test(
        &self,
        pkg: &ArchiveIdentifier,
        path: &Path,
        prefix: &Path,
        config: &TestConfiguration,
    ) -> Result<(), TestError> {
        let span = tracing::info_span!("Running perl test");
        let _guard = span.enter();

        let match_spec = MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
            ParseStrictness::Lenient,
        )?;

        let dependencies = vec!["perl".parse().unwrap(), match_spec];

        let test_prefix = prefix.join("test_env");
        create_environment(
            "test",
            &dependencies,
            config
                .host_platform
                .as_ref()
                .unwrap_or(&config.current_platform),
            &test_prefix,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        let mut imports = String::new();
        tracing::info!("Testing perl imports:\n");

        for module in &self.uses {
            writeln!(imports, "use {};", module)?;
            tracing::info!("  use {};", module);
        }
        tracing::info!("\n");

        let script = Script {
            content: ScriptContent::Command(imports.clone()),
            interpreter: Some("perl".into()),
            ..Script::default()
        };

        let test_folder = prefix.join("test_files");
        fs::create_dir_all(&test_folder)?;
        script
            .run_script(
                Default::default(),
                &test_folder,
                path,
                &test_prefix,
                None,
                None,
                None,
                config.debug,
            )
            .await
            .map_err(|e| TestError::TestFailed(e.to_string()))?;

        Ok(())
    }
}

impl CommandsTest {
    /// Execute the command test
    pub async fn run_test(
        &self,
        pkg: &ArchiveIdentifier,
        path: &Path,
        test_directory: &Path,
        config: &TestConfiguration,
        pkg_vars: &HashMap<String, String>,
    ) -> Result<(), TestError> {
        let deps = self.requirements.clone();

        let span = tracing::info_span!("Running script test for", recipe = pkg.to_string());
        let _guard = span.enter();

        let build_prefix = if !deps.build.is_empty() {
            tracing::info!("Installing build dependencies");
            let build_prefix = test_directory.join("test_build_env");
            let build_dependencies = deps
                .build
                .iter()
                .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
                .collect::<Result<Vec<_>, _>>()?;

            create_environment(
                "test",
                &build_dependencies,
                &config.current_platform,
                &build_prefix,
                &config.channels,
                &config.tool_configuration,
                config.channel_priority,
                config.solve_strategy,
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
            .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
            .collect::<Result<Vec<_>, _>>()?;

        // create environment with the test dependencies
        dependencies.push(MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
            ParseStrictness::Lenient,
        )?);

        let platform = config
            .host_platform
            .as_ref()
            .unwrap_or(&config.current_platform);

        let run_prefix = test_directory.join("test_run_env");
        create_environment(
            "test",
            &dependencies,
            platform,
            &run_prefix,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        let platform = Platform::current();
        let mut env_vars = env_vars::os_vars(&run_prefix, &platform);
        env_vars.retain(|key, _| key != ShellEnum::default().path_var(&platform));
        env_vars.extend(pkg_vars.iter().map(|(k, v)| (k.clone(), Some(v.clone()))));
        env_vars.insert(
            "PREFIX".to_string(),
            Some(run_prefix.to_string_lossy().to_string()),
        );

        // copy all test files to a temporary directory and set it as the working
        // directory
        let test_dir = test_directory.join("test");
        CopyDir::new(path, &test_dir).run().map_err(|e| {
            TestError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to copy test files: {}", e),
            ))
        })?;

        tracing::info!("Testing commands:");
        self.script
            .run_script(
                env_vars,
                &test_dir,
                path,
                &run_prefix,
                build_prefix.as_ref(),
                None,
                None,
                config.debug,
            )
            .await
            .map_err(|e| TestError::TestFailed(e.to_string()))?;

        Ok(())
    }
}

impl DownstreamTest {
    /// Execute the command test
    pub async fn run_test(
        &self,
        pkg: &ArchiveIdentifier,
        path: &Path,
        prefix: &Path,
        config: &TestConfiguration,
    ) -> Result<(), TestError> {
        let downstream_spec = self.downstream.clone();

        let span = tracing::info_span!("Running downstream test for", package = downstream_spec);
        let _guard = span.enter();

        // first try to resolve an environment with the downstream spec and our
        // current package
        let match_specs = [
            MatchSpec::from_str(&downstream_spec, ParseStrictness::Lenient)?,
            MatchSpec::from_str(
                format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
                ParseStrictness::Lenient,
            )?,
        ];

        let resolved = create_environment(
            "test",
            &match_specs,
            &config.current_platform,
            prefix,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await;

        match resolved {
            Ok(solution) => {
                let spec_name = match_specs[0].name.clone().expect("matchspec has a name");
                // we found a solution, so let's run the downstream test with that particular
                // package!
                let downstream_package = solution
                    .iter()
                    .find(|s| s.package_record.name == spec_name)
                    .ok_or_else(|| {
                        TestError::TestFailed(
                            "Could not find package in the resolved environment".to_string(),
                        )
                    })?;

                let temp_dir = tempfile::tempdir()?;
                let package_file = temp_dir.path().join(&downstream_package.file_name);

                if downstream_package.url.scheme() == "file" {
                    fs::copy(
                        downstream_package.url.to_file_path().unwrap(),
                        &package_file,
                    )?;
                } else {
                    let package_dl = reqwest::get(downstream_package.url.clone()).await.unwrap();
                    // write out the package to a temporary directory
                    let mut file = fs::File::create(&package_file)?;
                    let bytes = package_dl.bytes().await.unwrap();
                    file.write_all(&bytes)?;
                }

                // run the test with the downstream package
                tracing::info!("Running downstream test with {:?}", &package_file);
                run_test(path, config, Some(package_file.clone()))
                    .await
                    .inspect_err(|_| {
                        tracing::error!("Downstream test with {:?} failed", &package_file);
                    })?;
            }
            Err(e) => {
                // ignore the error
                tracing::warn!(
                    "Downstream test could not run. Environment might be unsolvable: {:?}",
                    e
                );
            }
        }

        Ok(())
    }
}

impl RTest {
    /// Execute the R test
    pub async fn run_test(
        &self,
        pkg: &ArchiveIdentifier,
        path: &Path,
        prefix: &Path,
        config: &TestConfiguration,
    ) -> Result<(), TestError> {
        let span = tracing::info_span!("Running R test");
        let _guard = span.enter();

        let match_spec = MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
            ParseStrictness::Lenient,
        )?;

        let dependencies = vec!["r-base".parse().unwrap(), match_spec];
        let test_prefix = prefix.join("test_env");
        create_environment(
            "test",
            &dependencies,
            config
                .host_platform
                .as_ref()
                .unwrap_or(&config.current_platform),
            &test_prefix,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        let mut libraries = String::new();
        tracing::info!("Testing R libraries:\n");

        for library in &self.libraries {
            writeln!(libraries, "library({})", library)?;
            tracing::info!("  library({})", library);
        }
        tracing::info!("\n");

        let script = Script {
            content: ScriptContent::Command(libraries.clone()),
            interpreter: Some("rscript".into()),
            ..Script::default()
        };

        let test_folder = prefix.join("test_files");
        fs::create_dir_all(&test_folder)?;
        script
            .run_script(
                Default::default(),
                &test_folder,
                path,
                &test_prefix,
                None,
                None,
                None,
                config.debug,
            )
            .await
            .map_err(|e| TestError::TestFailed(e.to_string()))?;

        Ok(())
    }
}
