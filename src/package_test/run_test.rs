//! Testing a package produced by rattler-build (or conda-build)
//!
//! Tests are part of the final package (under the `info/test` directory).
//! There are multiple test types:
//!
//! * `commands` - run a list of commands and check their exit code
//! * `imports` - import a list of modules and check if they can be imported
//! * `files` - check if a list of files exist

use fs_err as fs;
use rattler_conda_types::package::{IndexJson, PackageFile};
use rattler_conda_types::{Channel, ParseStrictness};
use std::collections::HashMap;
use std::fmt::Write as fmt_write;
use std::io::Write;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use dunce::canonicalize;
use rattler::package_cache::CacheKey;
use rattler_conda_types::{package::ArchiveIdentifier, MatchSpec, Platform};
use rattler_index::index;
use rattler_shell::{activation::ActivationError, shell::Shell, shell::ShellEnum};
use rattler_solve::{ChannelPriority, SolveStrategy};
use url::Url;

use crate::env_vars;
use crate::recipe::parser::{CommandsTest, DownstreamTest, Script, ScriptContent, TestType};
use crate::selectors::SelectorConfig;
use crate::source::copy_dir::CopyDir;
use crate::{recipe::parser::PythonTest, render::solver::create_environment, tool_configuration};

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
        env_vars.extend(pkg_vars.clone());
        env_vars.insert(
            "PREFIX".to_string(),
            environment.to_string_lossy().to_string(),
        );
        let tmp_dir = tempfile::tempdir()?;

        match self {
            Tests::Commands(path) => {
                let script = Script {
                    content: ScriptContent::Path(path.clone()),
                    ..Script::default()
                };

                // copy all test files to a temporary directory and set it as the working directory
                CopyDir::new(path, tmp_dir.path()).run().map_err(|e| {
                    TestError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to copy test files: {}", e),
                    ))
                })?;

                script
                    .run_script(env_vars, tmp_dir.path(), cwd, environment, None, None)
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
                    .run_script(env_vars, tmp_dir.path(), cwd, environment, None, None)
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
    /// The target platform. If not set it will be discovered from the index.json metadata.
    pub target_platform: Option<Platform>,
    /// The host platform for run-time dependencies. If not set it will be
    /// discovered from the index.json metadata.
    pub host_platform: Option<Platform>,
    /// If true, the test prefix will not be deleted after the test is run
    pub keep_test_prefix: bool,
    /// The channels to use for the test – do not forget to add the local build outputs channel
    /// if desired
    pub channels: Vec<Url>,
    /// The channel priority that is used to resolve dependencies
    pub channel_priority: ChannelPriority,
    /// The solve strategy to use when resolving dependencies
    pub solve_strategy: SolveStrategy,
    /// The tool configuration
    pub tool_configuration: tool_configuration::Configuration,
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

impl TestConfiguration {
    pub(crate) fn selector_config(&self, index_json: &IndexJson) -> SelectorConfig {
        SelectorConfig {
            target_platform: self.target_platform.unwrap_or(Platform::current()),
            host_platform: self.host_platform.unwrap_or(Platform::current()),
            build_platform: Platform::current(),
            hash: None, 
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
        }
    }
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
    std::fs::create_dir_all(&subdir)?;

    std::fs::copy(
        package_file,
        subdir.join(
            package_file
                .file_name()
                .ok_or(TestError::MissingPackageFileName)?,
        ),
    )?;

    // Also copy the downstream package if it exists
    if let Some(ref downstream_package) = downstream_package {
        std::fs::copy(
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

    // index the temporary channel
    index(tmp_repo.path(), Some(&target_platform))?;

    let cache_dir = rattler::default_cache_dir()?;

    let pkg = ArchiveIdentifier::try_from_path(package_file)
        .ok_or_else(|| TestError::TestFailed("could not get archive identifier".to_string()))?;

    // if the package is already in the cache, remove it. TODO make this based on SHA256 instead!
    let cache_key = CacheKey::from(pkg.clone());
    let package_folder = cache_dir.join("pkgs").join(cache_key.to_string());

    if package_folder.exists() {
        tracing::info!("Removing previously cached package {:?}", &package_folder);
        fs::remove_dir_all(&package_folder)?;
    }

    let prefix = canonicalize(&config.test_prefix)?;

    tracing::info!("Creating test environment in {:?}", prefix);

    let mut channels = config.channels.clone();
    channels.insert(0, Channel::from_directory(tmp_repo.path()).base_url);

    let host_platform = config.host_platform.unwrap_or_else(|| {
        if target_platform == Platform::NoArch {
            Platform::current()
        } else {
            target_platform
        }
    });

    let config = TestConfiguration {
        target_platform: Some(target_platform),
        host_platform: Some(host_platform),
        channels,
        ..config.clone()
    };

    tracing::info!("Collecting tests from {:?}", package_folder);

    rattler_package_streaming::fs::extract(package_file, &package_folder).map_err(|e| {
        tracing::error!("Failed to extract package: {:?}", e);
        TestError::TestFailed(format!("failed to extract package: {:?}", e))
    })?;

    let index_json = IndexJson::from_package_directory(&package_folder)?;
    let env = env_vars_from_package(&index_json);
    // extract package in place
    if package_folder.join("info/test").exists() {
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
    }

    if package_folder.join("info/tests/tests.yaml").exists() {
        let tests = fs::read_to_string(package_folder.join("info/tests/tests.yaml"))?;
        let tests: Vec<TestType> = serde_yaml::from_str(&tests)?;

        for test in tests {
            match test {
                TestType::Command(c) => {
                    c.run_test(&pkg, &package_folder, &prefix, &config, &env)
                        .await?
                }
                TestType::Python { python } => {
                    python
                        .run_test(&pkg, &package_folder, &prefix, &config)
                        .await?
                }
                TestType::Downstream(downstream) if downstream_package.is_none() => {
                    downstream
                        .run_test(&pkg, package_file, &prefix, &config)
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
        }

        tracing::info!(
            "{} all tests passed!",
            console::style(console::Emoji("✔", "")).green()
        );
    }

    if prefix.exists() {
        fs::remove_dir_all(prefix)?;
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
        let span = tracing::info_span!("Running python test");
        let _guard = span.enter();

        let match_spec = MatchSpec::from_str(
            format!("{}={}={}", pkg.name, pkg.version, pkg.build_string).as_str(),
            ParseStrictness::Lenient,
        )?;
        let mut dependencies = vec![match_spec];
        if self.pip_check {
            dependencies.push(MatchSpec::from_str("pip", ParseStrictness::Strict).unwrap());
        }

        create_environment(
            "test",
            &dependencies,
            &Platform::current(),
            prefix,
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

        let tmp_dir = tempfile::tempdir()?;
        script
            .run_script(Default::default(), tmp_dir.path(), path, prefix, None, None)
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
                .run_script(Default::default(), path, path, prefix, None, None)
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

impl CommandsTest {
    /// Execute the command test
    pub async fn run_test(
        &self,
        pkg: &ArchiveIdentifier,
        path: &Path,
        prefix: &Path,
        config: &TestConfiguration,
        pkg_vars: &HashMap<String, String>,
    ) -> Result<(), TestError> {
        let deps = self.requirements.clone();

        let span = tracing::info_span!("Running script test");
        let _guard = span.enter();

        let build_env = if !deps.build.is_empty() {
            tracing::info!("Installing build dependencies");
            let build_prefix = prefix.join("bld");
            let platform = Platform::current();
            let build_dependencies = deps
                .build
                .iter()
                .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
                .collect::<Result<Vec<_>, _>>()?;

            create_environment(
                "test",
                &build_dependencies,
                &platform,
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

        let platform = config.host_platform.unwrap_or_else(Platform::current);

        let run_env = prefix.join("run");
        create_environment(
            "test",
            &dependencies,
            &platform,
            &run_env,
            &config.channels,
            &config.tool_configuration,
            config.channel_priority,
            config.solve_strategy,
        )
        .await
        .map_err(TestError::TestEnvironmentSetup)?;

        let platform = Platform::current();
        let mut env_vars = env_vars::os_vars(prefix, &platform);
        env_vars.retain(|key, _| key != ShellEnum::default().path_var(&platform));
        env_vars.extend(pkg_vars.clone());
        env_vars.insert("PREFIX".to_string(), run_env.to_string_lossy().to_string());

        // copy all test files to a temporary directory and set it as the working directory
        let tmp_dir = tempfile::tempdir()?;
        CopyDir::new(path, tmp_dir.path()).run().map_err(|e| {
            TestError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to copy test files: {}", e),
            ))
        })?;

        // Create a jinja environment to render the script commands
        let selector_config = config.selector_config();
        let mut jinja = Jinja::new(selector_config.clone());
        for (k, v) in self.recipe.context.iter() {
            jinja
                .context_mut()
                .insert(k.clone(), Value::from_safe_string(v.clone()));
        }


        tracing::info!("Testing commands:");
        self.script
            .run_script(
                env_vars,
                tmp_dir.path(),
                path,
                &run_env,
                build_env.as_ref(),
                None, // Add jinja env here! 
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

        let target_platform = config.target_platform.unwrap_or_else(Platform::current);

        let resolved = create_environment(
            "test",
            &match_specs,
            &target_platform,
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
                // we found a solution, so let's run the downstream test with that particular package!
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
                    .map_err(|e| {
                        tracing::error!("Downstream test with {:?} failed", &package_file);
                        e
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
