//! This is the main entry point for the `rattler-build` binary.

use clap::{arg, crate_version, CommandFactory, Parser};

use clap_verbosity_flag::{InfoLevel, Verbosity};
use dunce::canonicalize;
use fs_err as fs;
use miette::IntoDiagnostic;
use rattler_conda_types::{package::ArchiveType, Platform};
use rattler_package_streaming::write::CompressionLevel;
use std::{
    collections::BTreeMap,
    env::current_dir,
    path::PathBuf,
    str::{self, FromStr},
    sync::{Arc, Mutex},
};

use url::Url;

use rattler_build::{
    build::run_build,
    console_utils::{init_logging, Color, LogStyle, LoggingOutputHandler},
    hash::HashInfo,
    metadata::{
        BuildConfiguration, BuildSummary, Directories, PackageIdentifier, PackagingSettings,
    },
    package_test::{self, TestConfiguration},
    recipe::{
        parser::{find_outputs_from_src, Recipe},
        ParsingError,
    },
    recipe_generator::{generate_recipe, GenerateRecipeOpts},
    selectors::SelectorConfig,
    system_tools::SystemTools,
    tool_configuration,
    variant_config::{ParseErrors, VariantConfig},
};

mod rebuild;
mod upload;

#[derive(Parser)]
enum SubCommands {
    /// Build a package
    Build(BuildOpts),

    /// Test a package
    Test(TestOpts),

    /// Rebuild a package
    Rebuild(RebuildOpts),

    /// Upload a package
    Upload(UploadOpts),

    /// Generate shell completion script
    Completion(ShellCompletion),

    /// Generate a recipe from PyPI or CRAN
    GenerateRecipe(GenerateRecipeOpts),
}

#[derive(Parser)]
struct ShellCompletion {
    #[arg(short, long)]
    shell: Option<clap_complete::Shell>,
}

#[derive(Parser)]
#[clap(version = crate_version!())]
struct App {
    #[clap(subcommand)]
    subcommand: Option<SubCommands>,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,

    /// Logging style
    #[clap(
        long,
        env = "RATTLER_BUILD_LOG_STYLE",
        default_value = "fancy",
        global = true
    )]
    log_style: LogStyle,

    /// Enable or disable colored output from rattler-build.
    /// Also honors the `CLICOLOR` and `CLICOLOR_FORCE` environment variable.
    #[clap(
        long,
        env = "RATTLER_BUILD_COLOR",
        default_value = "auto",
        global = true
    )]
    color: Color,
}

/// Common opts that are shared between [`Rebuild`] and [`Build`]` subcommands
#[derive(Parser)]
struct CommonOpts {
    /// Output directory for build artifacts. Defaults to `./output`.
    #[clap(long, env = "CONDA_BLD_PATH")]
    output_dir: Option<PathBuf>,

    /// Enable support for repodata.json.zst
    #[clap(long, env = "RATTLER_ZSTD", default_value = "true", hide = true)]
    use_zstd: bool,

    /// Enable support for repodata.json.bz2
    #[clap(long, env = "RATTLER_BZ2", default_value = "true", hide = true)]
    use_bz2: bool,

    /// Enable experimental features
    #[arg(long, env = "RATTLER_BUILD_EXPERIMENTAL")]
    experimental: bool,

    /// Path to an auth-file to read authentication information from
    #[clap(long, env = "RATTLER_AUTH_FILE", hide = true)]
    auth_file: Option<PathBuf>,
}

/// Container for the CLI package format and compression level
#[derive(Clone, PartialEq, Eq, Debug)]
struct PackageFormatAndCompression {
    /// The archive type that is selected
    pub archive_type: ArchiveType,
    /// The compression level that is selected
    pub compression_level: CompressionLevel,
}

// deserializer for the package format and compression level
impl FromStr for PackageFormatAndCompression {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split(':');
        let package_format = split.next().ok_or("invalid")?;

        let compression = split.next().unwrap_or("default");

        // remove all non-alphanumeric characters
        let package_format = package_format
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>();

        let archive_type = match package_format.to_lowercase().as_str() {
            "tarbz2" => ArchiveType::TarBz2,
            "conda" => ArchiveType::Conda,
            _ => return Err(format!("Unknown package format: {}", package_format)),
        };

        let compression_level = match compression {
            "max" | "highest" => CompressionLevel::Highest,
            "default" | "normal" => CompressionLevel::Default,
            "fast" | "lowest" | "min" => CompressionLevel::Lowest,
            number if number.parse::<i32>().is_ok() => {
                let number = number.parse::<i32>().unwrap_or_default();
                match archive_type {
                    ArchiveType::TarBz2 => {
                        if !(1..=9).contains(&number) {
                            return Err("Compression level for .tar.bz2 must be between 1 and 9"
                                .to_string());
                        }
                    }
                    ArchiveType::Conda => {
                        if !(-7..=22).contains(&number) {
                            return Err(
                                "Compression level for conda packages (zstd) must be between -7 and 22".to_string()
                            );
                        }
                    }
                }
                CompressionLevel::Numeric(number)
            }
            _ => return Err(format!("Unknown compression level: {}", compression)),
        };

        Ok(PackageFormatAndCompression {
            archive_type,
            compression_level,
        })
    }
}

#[derive(Parser)]
struct BuildOpts {
    /// The recipe file or directory containing `recipe.yaml`. Defaults to the current directory.
    #[arg(short, long, default_value = ".")]
    recipe: PathBuf,

    /// The target platform for the build.
    #[arg(long)]
    target_platform: Option<String>,

    /// Add the channels needed for the recipe using this option. For more then one channel use it multiple times.
    /// The default channel is `conda-forge`.
    #[arg(short = 'c', long)]
    channel: Option<Vec<String>>,

    /// Variant configuration files for the build.
    #[arg(short = 'm', long)]
    variant_config: Vec<PathBuf>,

    /// Render the recipe files without executing the build.
    #[arg(long)]
    render_only: bool,

    /// Keep intermediate build artifacts after the build.
    #[arg(long)]
    keep_build: bool,

    /// Don't use build id(timestamp) when creating build directory name. Defaults to `false`.
    #[arg(long)]
    no_build_id: bool,

    /// The package format to use for the build. Can be one of `tar-bz2` or `conda`.
    /// You can also add a compression level to the package format, e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to 22).
    #[arg(long, default_value = "conda")]
    package_format: PackageFormatAndCompression,

    #[arg(long)]
    /// The number of threads to use for compression (only relevant when also using `--package-format conda`)
    compression_threads: Option<u32>,

    /// Do not store the recipe in the final package
    #[arg(long)]
    no_include_recipe: bool,

    /// Do not run tests after building
    #[arg(long, default_value = "false")]
    no_test: bool,

    /// Do not force colors in the output of the build script
    #[arg(long, default_value = "true")]
    color_build_log: bool,

    #[clap(flatten)]
    common: CommonOpts,
}

#[derive(Parser)]
struct TestOpts {
    /// Channels to use when testing
    #[arg(short = 'c', long)]
    channel: Option<Vec<String>>,

    /// The package file to test
    #[arg(short, long)]
    package_file: PathBuf,

    #[clap(flatten)]
    common: CommonOpts,
}

#[derive(Parser)]
struct RebuildOpts {
    /// The package file to rebuild
    #[arg(short, long)]
    package_file: PathBuf,

    /// Do not run tests after building
    #[arg(long, default_value = "false")]
    no_test: bool,

    #[clap(flatten)]
    common: CommonOpts,
}

#[derive(Parser)]
struct UploadOpts {
    /// The package file to upload
    #[arg(global = true, required = false)]
    package_files: Vec<PathBuf>,

    /// The server type
    #[clap(subcommand)]
    server_type: ServerType,

    #[clap(flatten)]
    common: CommonOpts,
}

#[derive(Clone, Debug, PartialEq, Parser)]
enum ServerType {
    Quetz(QuetzOpts),
    Artifactory(ArtifactoryOpts),
    Prefix(PrefixOpts),
    Anaconda(AnacondaOpts),
    #[clap(hide = true)]
    CondaForge(CondaForgeOpts),
}

#[derive(Clone, Debug, PartialEq, Parser)]
/// Options for uploading to a Quetz server.
/// Authentication is used from the keychain / auth-file.
struct QuetzOpts {
    /// The URL to your Quetz server
    #[arg(short, long, env = "QUETZ_SERVER_URL")]
    url: Url,

    /// The URL to your channel
    #[arg(short, long, env = "QUETZ_CHANNEL")]
    channel: String,

    /// The Quetz API key, if none is provided, the token is read from the keychain / auth-file
    #[arg(short, long, env = "QUETZ_API_KEY")]
    api_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Parser)]
/// Options for uploading to a Artifactory channel.
/// Authentication is used from the keychain / auth-file.
struct ArtifactoryOpts {
    /// The URL to your Artifactory server
    #[arg(short, long, env = "ARTIFACTORY_SERVER_URL")]
    url: Url,

    /// The URL to your channel
    #[arg(short, long, env = "ARTIFACTORY_CHANNEL")]
    channel: String,

    /// Your Artifactory username
    #[arg(short = 'r', long, env = "ARTIFACTORY_USERNAME")]
    username: Option<String>,

    /// Your Artifactory password
    #[arg(short, long, env = "ARTIFACTORY_PASSWORD")]
    password: Option<String>,
}

/// Options for uploading to a prefix.dev server.
/// Authentication is used from the keychain / auth-file
#[derive(Clone, Debug, PartialEq, Parser)]
struct PrefixOpts {
    /// The URL to the prefix.dev server (only necessary for self-hosted instances)
    #[arg(
        short,
        long,
        env = "PREFIX_SERVER_URL",
        default_value = "https://prefix.dev"
    )]
    url: Url,

    /// The channel to upload the package to
    #[arg(short, long, env = "PREFIX_CHANNEL")]
    channel: String,

    /// The prefix.dev API key, if none is provided, the token is read from the keychain / auth-file
    #[arg(short, long, env = "PREFIX_API_KEY")]
    api_key: Option<String>,
}

/// Options for uploading to a Anaconda.org server
#[derive(Clone, Debug, PartialEq, Parser)]
struct AnacondaOpts {
    /// The owner of the distribution (e.g. conda-forge or your username)
    #[arg(short, long, env = "ANACONDA_OWNER")]
    owner: String,

    /// The channel / label to upload the package to (e.g. main / rc)
    #[arg(short, long, env = "ANACONDA_CHANNEL", default_value = "main", num_args = 1..)]
    channel: Vec<String>,

    /// The Anaconda API key, if none is provided, the token is read from the keychain / auth-file
    #[arg(short, long, env = "ANACONDA_API_KEY")]
    api_key: Option<String>,

    /// The URL to the Anaconda server
    #[arg(
        short,
        long,
        env = "ANACONDA_SERVER_URL",
        default_value = "https://api.anaconda.org"
    )]
    url: Url,

    /// Replace files on conflict
    #[arg(long, short, env = "ANACONDA_FORCE", default_value = "false")]
    force: bool,
}

/// Options for uploading to conda-forge
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct CondaForgeOpts {
    /// The Anaconda API key
    #[arg(long, env = "STAGING_BINSTAR_TOKEN", required = true)]
    staging_token: String,

    /// The feedstock name
    #[arg(long, env = "FEEDSTOCK_NAME", required = true)]
    feedstock: String,

    /// The feedstock token
    #[arg(long, env = "FEEDSTOCK_TOKEN", required = true)]
    feedstock_token: String,

    /// The staging channel name
    #[arg(long, env = "STAGING_CHANNEL", default_value = "cf-staging")]
    staging_channel: String,

    /// The Anaconda Server URL
    #[arg(
        long,
        env = "ANACONDA_SERVER_URL",
        default_value = "https://api.anaconda.org"
    )]
    anaconda_url: Url,

    /// The validation endpoint url
    #[arg(
        long,
        env = "VALIDATION_ENDPOINT",
        default_value = "https://conda-forge.herokuapp.com/feedstock-outputs/copy"
    )]
    validation_endpoint: Url,

    /// Post comment on promotion failure
    #[arg(long, env = "POST_COMMENT_ON_ERROR", default_value = "true")]
    post_comment_on_error: bool,

    /// The CI provider
    #[arg(long, env = "CI")]
    provider: Option<String>,

    /// Dry run, don't actually upload anything
    #[arg(long, env = "DRY_RUN", default_value = "false")]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = App::parse();

    let fancy_log_handler =
        init_logging(&args.log_style, &args.verbose, &args.color).into_diagnostic()?;

    match args.subcommand {
        Some(SubCommands::Completion(ShellCompletion { shell })) => {
            let mut cmd = App::command();
            fn print_completions<G: clap_complete::Generator>(gen: G, cmd: &mut clap::Command) {
                clap_complete::generate(
                    gen,
                    cmd,
                    cmd.get_name().to_string(),
                    &mut std::io::stdout(),
                );
            }
            let shell = shell
                .or(clap_complete::Shell::from_env())
                .unwrap_or(clap_complete::Shell::Bash);
            print_completions(shell, &mut cmd);
            Ok(())
        }
        Some(SubCommands::Build(args)) => run_build_from_args(args, fancy_log_handler).await,
        Some(SubCommands::Test(args)) => run_test_from_args(args, fancy_log_handler).await,
        Some(SubCommands::Rebuild(args)) => rebuild_from_args(args, fancy_log_handler).await,
        Some(SubCommands::Upload(args)) => upload_from_args(args).await,
        Some(SubCommands::GenerateRecipe(args)) => generate_recipe(args).await,
        None => {
            _ = App::command().print_long_help();
            Ok(())
        }
    }
}

async fn run_test_from_args(
    args: TestOpts,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    let package_file = canonicalize(args.package_file).into_diagnostic()?;
    let client = tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file);

    let tempdir = tempfile::tempdir().into_diagnostic()?;

    let test_options = TestConfiguration {
        test_prefix: tempdir.path().to_path_buf(),
        target_platform: None,
        keep_test_prefix: false,
        channels: args
            .channel
            .unwrap_or_else(|| vec!["conda-forge".to_string()]),
        tool_configuration: tool_configuration::Configuration {
            client,
            fancy_log_handler,
            // duplicate from `keep_test_prefix`?
            no_clean: false,
            ..Default::default()
        },
    };

    let package_name = package_file
        .file_name()
        .ok_or_else(|| miette::miette!("Could not get file name from package file"))?
        .to_string_lossy()
        .to_string();

    let span = tracing::info_span!("Running tests for ", recipe = %package_name);
    let _enter = span.enter();
    package_test::run_test(&package_file, &test_options)
        .await
        .into_diagnostic()?;

    Ok(())
}

async fn run_build_from_args(
    args: BuildOpts,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    let recipe_path = canonicalize(&args.recipe);
    if let Err(e) = &recipe_path {
        match e.kind() {
            std::io::ErrorKind::NotFound => {
                return Err(miette::miette!(
                    "The file {} could not be found.",
                    args.recipe.to_string_lossy()
                ));
            }
            std::io::ErrorKind::PermissionDenied => {
                return Err(miette::miette!(
                    "Permission denied when trying to access the file {}.",
                    args.recipe.to_string_lossy()
                ));
            }
            _ => {
                return Err(miette::miette!(
                    "An unknown error occurred while trying to access the file {}: {:?}",
                    args.recipe.to_string_lossy(),
                    e
                ));
            }
        }
    }
    let mut recipe_path = recipe_path.into_diagnostic()?;

    // If the recipe_path is a directory, look for "recipe.yaml" in the directory.
    if recipe_path.is_dir() {
        let recipe_yaml_path = recipe_path.join("recipe.yaml");
        if recipe_yaml_path.exists() && recipe_yaml_path.is_file() {
            recipe_path = recipe_yaml_path;
        } else {
            return Err(miette::miette!(
                "'recipe.yaml' not found in the directory {}",
                args.recipe.to_string_lossy()
            ));
        }
    }

    let output_dir = args
        .common
        .output_dir
        .unwrap_or(current_dir().into_diagnostic()?.join("output"));
    if output_dir.starts_with(
        recipe_path
            .parent()
            .expect("Could not get parent of recipe"),
    ) {
        return Err(miette::miette!(
            "The output directory cannot be in the recipe directory.\nThe current output directory is: {}\nSelect a different output directory with the --output-dir option or set the CONDA_BLD_PATH environment variable"
        , output_dir.to_string_lossy()));
    }

    let recipe_text = fs::read_to_string(&recipe_path).into_diagnostic()?;

    let host_platform = if let Some(target_platform) = args.target_platform {
        Platform::from_str(&target_platform).into_diagnostic()?
    } else {
        Platform::current()
    };

    let selector_config = SelectorConfig {
        // We ignore noarch here
        target_platform: host_platform,
        hash: None,
        build_platform: Platform::current(),
        variant: BTreeMap::new(),
        experimental: args.common.experimental,
    };

    let span = tracing::info_span!("Finding outputs from recipe");
    tracing::info!("Target platform: {}", host_platform);
    let enter = span.enter();
    // First find all outputs from the recipe
    let outputs = find_outputs_from_src(&recipe_text)?;

    let variant_config =
        VariantConfig::from_files(&args.variant_config, &selector_config).into_diagnostic()?;

    let outputs_and_variants =
        variant_config.find_variants(&outputs, &recipe_text, &selector_config)?;

    tracing::info!("Found {} variants\n", outputs_and_variants.len());
    for discovered_output in &outputs_and_variants {
        tracing::info!(
            "Build variant: {}-{}-{}",
            discovered_output.name,
            discovered_output.version,
            discovered_output.build_string
        );

        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Variant", "Version"]);
        for (key, value) in discovered_output.used_vars.iter() {
            table.add_row(vec![key, value]);
        }
        tracing::info!("{}\n", table);
    }
    drop(enter);

    let client = tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file);

    let tool_config = tool_configuration::Configuration {
        client,
        fancy_log_handler: fancy_log_handler.clone(),
        no_clean: args.keep_build,
        no_test: args.no_test,
        use_zstd: args.common.use_zstd,
        use_bz2: args.common.use_bz2,
    };

    let mut subpackages = BTreeMap::new();
    let mut outputs = Vec::new();
    for discovered_output in outputs_and_variants {
        let hash =
            HashInfo::from_variant(&discovered_output.used_vars, &discovered_output.noarch_type);

        let selector_config = SelectorConfig {
            variant: discovered_output.used_vars.clone(),
            hash: Some(hash.clone()),
            target_platform: selector_config.target_platform,
            build_platform: selector_config.build_platform,
            experimental: args.common.experimental,
        };

        let recipe =
            Recipe::from_node(&discovered_output.node, selector_config).map_err(|err| {
                let errs: ParseErrors = err
                    .into_iter()
                    .map(|err| ParsingError::from_partial(&recipe_text, err))
                    .collect::<Vec<ParsingError>>()
                    .into();
                errs
            })?;

        if recipe.build().skip() {
            tracing::info!(
                "Skipping build for variant: {:#?}",
                discovered_output.used_vars
            );
            continue;
        }

        subpackages.insert(
            recipe.package().name().clone(),
            PackageIdentifier {
                name: recipe.package().name().clone(),
                version: recipe.package().version().to_owned(),
                build_string: recipe
                    .build()
                    .string()
                    .expect("Shouldn't be unset, needs major refactoring, for handling this better")
                    .to_owned(),
            },
        );

        let name = recipe.package().name().clone();
        // Add the channels from the args and by default always conda-forge
        let channels = args
            .channel
            .clone()
            .unwrap_or_else(|| vec!["conda-forge".to_string()]);

        let timestamp = chrono::Utc::now();

        let output = rattler_build::metadata::Output {
            recipe,
            build_configuration: BuildConfiguration {
                target_platform: discovered_output.target_platform,
                host_platform,
                build_platform: Platform::current(),
                hash,
                variant: discovered_output.used_vars.clone(),
                directories: Directories::create(
                    name.as_normalized(),
                    &recipe_path,
                    &output_dir,
                    args.no_build_id,
                    &timestamp,
                )
                .into_diagnostic()?,
                channels,
                timestamp,
                subpackages: subpackages.clone(),
                packaging_settings: PackagingSettings::from_args(
                    args.package_format.archive_type,
                    args.package_format.compression_level,
                    args.compression_threads,
                ),
                store_recipe: !args.no_include_recipe,
                force_colors: args.color_build_log && console::colors_enabled(),
            },
            finalized_dependencies: None,
            finalized_sources: None,
            system_tools: SystemTools::new(),
            build_summary: Arc::new(Mutex::new(BuildSummary::default())),
        };

        if args.render_only {
            let resolved = output
                .resolve_dependencies(&tool_config)
                .await
                .into_diagnostic()?;
            println!("{}", serde_json::to_string_pretty(&resolved).unwrap());
            continue;
        }

        let output = match run_build(output, tool_config.clone()).await {
            Ok((output, _archive)) => {
                output.record_build_end();
                output
            }
            Err(e) => {
                tracing::error!("Error building package: {}", e);
                return Err(e);
            }
        };
        outputs.push(output);
    }

    let span = tracing::info_span!("Build summary");
    let _enter = span.enter();
    for output in outputs {
        // print summaries for each output
        let _ = output.log_build_summary().map_err(|e| {
            tracing::error!("Error writing build summary: {}", e);
            e
        });
    }

    Ok(())
}

async fn rebuild_from_args(
    args: RebuildOpts,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    tracing::info!("Rebuilding {}", args.package_file.to_string_lossy());
    // we extract the recipe folder from the package file (info/recipe/*)
    // and then run the rendered recipe with the same arguments as the original build
    let temp_folder = tempfile::tempdir().into_diagnostic()?;

    rebuild::extract_recipe(&args.package_file, temp_folder.path()).into_diagnostic()?;

    let temp_dir = temp_folder.into_path();

    tracing::info!("Extracted recipe to: {:?}", temp_dir);

    let rendered_recipe =
        fs::read_to_string(temp_dir.join("rendered_recipe.yaml")).into_diagnostic()?;

    let mut output: rattler_build::metadata::Output =
        serde_yaml::from_str(&rendered_recipe).into_diagnostic()?;

    // set recipe dir to the temp folder
    output.build_configuration.directories.recipe_dir = temp_dir;

    // create output dir and set it in the config
    let output_dir = args
        .common
        .output_dir
        .unwrap_or(current_dir().into_diagnostic()?.join("output"));

    fs::create_dir_all(&output_dir).into_diagnostic()?;
    output.build_configuration.directories.output_dir =
        canonicalize(output_dir).into_diagnostic()?;

    let client = tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file);

    let tool_config = tool_configuration::Configuration {
        client,
        fancy_log_handler,
        no_clean: true,
        no_test: args.no_test,
        use_zstd: args.common.use_zstd,
        use_bz2: args.common.use_bz2,
    };

    output
        .build_configuration
        .directories
        .recreate_directories()
        .into_diagnostic()?;

    run_build(output, tool_config.clone()).await?;

    Ok(())
}

async fn upload_from_args(args: UploadOpts) -> miette::Result<()> {
    if args.package_files.is_empty() {
        return Err(miette::miette!("No package files were provided."));
    }

    for package_file in &args.package_files {
        if ArchiveType::try_from(package_file).is_none() {
            return Err(miette::miette!(
                "The file {} does not appear to be a conda package.",
                package_file.to_string_lossy()
            ));
        }
    }

    let store = tool_configuration::get_auth_store(args.common.auth_file);

    match args.server_type {
        ServerType::Quetz(quetz_opts) => {
            upload::upload_package_to_quetz(
                &store,
                quetz_opts.api_key,
                &args.package_files,
                quetz_opts.url,
                quetz_opts.channel,
            )
            .await?;
        }
        ServerType::Artifactory(artifactory_opts) => {
            upload::upload_package_to_artifactory(
                &store,
                artifactory_opts.username,
                artifactory_opts.password,
                &args.package_files,
                artifactory_opts.url,
                artifactory_opts.channel,
            )
            .await?;
        }
        ServerType::Prefix(prefix_opts) => {
            upload::upload_package_to_prefix(
                &store,
                prefix_opts.api_key,
                &args.package_files,
                prefix_opts.url,
                prefix_opts.channel,
            )
            .await?;
        }
        ServerType::Anaconda(anaconda_opts) => {
            upload::upload_package_to_anaconda(
                &store,
                anaconda_opts.api_key,
                &args.package_files,
                anaconda_opts.url,
                anaconda_opts.owner,
                anaconda_opts.channel,
                anaconda_opts.force,
            )
            .await?;
        }
        ServerType::CondaForge(conda_forge_opts) => {
            upload::conda_forge::upload_packages_to_conda_forge(
                conda_forge_opts,
                &args.package_files,
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::PackageFormatAndCompression;
    use rattler_conda_types::package::ArchiveType;
    use rattler_package_streaming::write::CompressionLevel;
    use std::str::FromStr;

    #[test]
    fn test_parse_packaging() {
        let package_format = PackageFormatAndCompression::from_str("tar-bz2").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Default
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Default
            }
        );

        let package_format = PackageFormatAndCompression::from_str("tar-bz2:1").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Numeric(1)
            }
        );

        let package_format = PackageFormatAndCompression::from_str(".tar.bz2:max").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Highest
            }
        );

        let package_format = PackageFormatAndCompression::from_str("tarbz2:5").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Numeric(5)
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:1").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Numeric(1)
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:max").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Highest
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:-5").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Numeric(-5)
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:fast").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Lowest
            }
        );
    }
}
