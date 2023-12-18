//! This is the main entry point for the `rattler-build` binary.

use clap::{arg, crate_version, Parser};

use clap_verbosity_flag::{InfoLevel, Verbosity};
use dunce::canonicalize;
use fs_err as fs;
use indicatif::MultiProgress;
use miette::IntoDiagnostic;
use rattler_conda_types::{package::ArchiveType, Platform};
use rattler_networking::AuthenticatedClient;
use std::{
    collections::BTreeMap,
    env::current_dir,
    path::PathBuf,
    str::{self, FromStr},
    sync::Arc,
};
use tracing_subscriber::{
    filter::{Directive, ParseError},
    fmt,
    prelude::*,
    EnvFilter,
};
use url::Url;

use rattler_build::{
    build::run_build,
    hash::HashInfo,
    metadata::{BuildConfiguration, Directories, PackageIdentifier},
    recipe::{parser::Recipe, ParsingError},
    selectors::SelectorConfig,
    test::{self, TestConfiguration},
    tool_configuration,
    variant_config::{ParseErrors, VariantConfig},
};

mod console_utils;
mod rebuild;
mod upload;

use crate::console_utils::{IndicatifWriter, TracingFormatter};

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
}

#[derive(Parser)]
#[clap(version = crate_version!())]
struct App {
    #[clap(subcommand)]
    subcommand: SubCommands,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[derive(clap::ValueEnum, Clone)]
enum PackageFormat {
    TarBz2,
    Conda,
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

    /// Path to an auth-file to read authentication information from
    #[clap(long, env = "RATTLER_AUTH_FILE", hide = true)]
    auth_file: Option<PathBuf>,
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

    /// The package format to use for the build.
    /// Defaults to `.tar.bz2`.
    #[arg(long, default_value = "tar-bz2")]
    package_format: PackageFormat,

    /// Do not store the recipe in the final package
    #[arg(long)]
    no_include_recipe: bool,

    /// Do not run tests after building
    #[arg(long, default_value = "false")]
    no_test: bool,

    /// Do not force colors in the output of the build script
    #[arg(long, default_value = "false")]
    no_force_colors: bool,

    #[clap(flatten)]
    common: CommonOpts,
}

#[derive(Parser)]
struct TestOpts {
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
    #[clap(short, long)]
    package_file: PathBuf,

    /// The server type
    #[clap(subcommand)]
    server_type: ServerType,

    #[clap(flatten)]
    common: CommonOpts,
}

#[derive(Clone, Debug, PartialEq, Parser)]
enum ServerType {
    Quetz(QuetzOpts),
}

#[derive(Clone, Debug, PartialEq, Parser)]
/// Options for uploading to a Quetz server
/// Authentication is used from the keychain / auth-file
struct QuetzOpts {
    /// The URL to your Quetz server
    #[arg(short, long)]
    url: Url,

    /// The URL to your channel
    #[arg(short, long)]
    channel: String,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = App::parse();

    let multi_progress = MultiProgress::new();

    // Setup tracing subscriber
    tracing_subscriber::registry()
        .with(get_default_env_filter(args.verbose.log_level_filter()).into_diagnostic()?)
        .with(
            fmt::layer()
                .with_writer(IndicatifWriter::new(multi_progress.clone()))
                .event_format(TracingFormatter),
        )
        .init();

    match args.subcommand {
        SubCommands::Build(args) => run_build_from_args(args, multi_progress).await,
        SubCommands::Test(args) => run_test_from_args(args).await,
        SubCommands::Rebuild(args) => rebuild_from_args(args).await,
        SubCommands::Upload(args) => upload_from_args(args).await,
    }
}

fn get_auth_store(auth_file: Option<PathBuf>) -> rattler_networking::AuthenticationStorage {
    match auth_file {
        Some(auth_file) => {
            let mut store = rattler_networking::AuthenticationStorage::new();
            store.add_backend(Arc::from(
                rattler_networking::authentication_storage::backends::file::FileStorage::new(
                    auth_file,
                ),
            ));
            store
        }
        None => rattler_networking::AuthenticationStorage::default(),
    }
}

async fn run_test_from_args(args: TestOpts) -> miette::Result<()> {
    let package_file = canonicalize(args.package_file).into_diagnostic()?;
    let test_prefix = PathBuf::from("test-prefix");
    fs::create_dir_all(&test_prefix).into_diagnostic()?;

    let test_options = TestConfiguration {
        test_prefix,
        target_platform: Some(Platform::current()),
        keep_test_prefix: false,
        channels: vec!["conda-forge".to_string(), "./output".to_string()],
    };

    let client = AuthenticatedClient::from_client(
        reqwest::Client::builder()
            .no_gzip()
            .build()
            .expect("failed to create client"),
        get_auth_store(args.common.auth_file),
    );

    let global_configuration = tool_configuration::Configuration {
        client,
        multi_progress_indicator: MultiProgress::new(),
        no_clean: test_options.keep_test_prefix,
        ..Default::default()
    };

    test::run_test(&package_file, &test_options, &global_configuration)
        .await
        .into_diagnostic()?;

    Ok(())
}

async fn run_build_from_args(args: BuildOpts, multi_progress: MultiProgress) -> miette::Result<()> {
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
        tracing::info!("No target platform specified, using current platform");
        Platform::current()
    };

    let selector_config = SelectorConfig {
        // We ignore noarch here
        target_platform: host_platform,
        hash: None,
        build_platform: Platform::current(),
        variant: BTreeMap::new(),
    };

    let variant_config =
        VariantConfig::from_files(&args.variant_config, &selector_config).into_diagnostic()?;

    let outputs_and_variants = variant_config.find_variants(&recipe_text, &selector_config)?;

    tracing::info!("Found variants:\n");
    for discovered_output in &outputs_and_variants {
        tracing::info!(
            "{}-{}-{}",
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

    let client = AuthenticatedClient::from_client(
        reqwest::Client::builder()
            .no_gzip()
            .build()
            .expect("failed to create client"),
        get_auth_store(args.common.auth_file),
    );

    let tool_config = tool_configuration::Configuration {
        client,
        multi_progress_indicator: multi_progress,
        no_clean: args.keep_build,
        no_test: args.no_test,
        use_zstd: args.common.use_zstd,
        use_bz2: args.common.use_bz2,
    };

    let mut subpackages = BTreeMap::new();
    for discovered_output in outputs_and_variants {
        let hash =
            HashInfo::from_variant(&discovered_output.used_vars, &discovered_output.noarch_type);

        let selector_config = SelectorConfig {
            variant: discovered_output.used_vars.clone(),
            hash: Some(hash.clone()),
            target_platform: selector_config.target_platform,
            build_platform: selector_config.build_platform,
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

        if args.render_only {
            tracing::info!(
                "Name: {} {}",
                recipe.package().name().as_normalized(),
                recipe.package().version()
            );
            tracing::info!("Variant: {:#?}", discovered_output.used_vars);
            tracing::info!("Hash: {:#?}", recipe.build().string());
            tracing::info!("Skip?: {}\n", recipe.build().skip());
            continue;
        }

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
                package_format: match args.package_format {
                    PackageFormat::TarBz2 => ArchiveType::TarBz2,
                    PackageFormat::Conda => ArchiveType::Conda,
                },
                store_recipe: !args.no_include_recipe,
                force_colors: !args.no_force_colors,
            },
            finalized_dependencies: None,
        };

        run_build(&output, tool_config.clone()).await?;
    }

    Ok(())
}

async fn rebuild_from_args(args: RebuildOpts) -> miette::Result<()> {
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

    let tool_config = tool_configuration::Configuration {
        client: AuthenticatedClient::default(),
        multi_progress_indicator: MultiProgress::new(),
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

    run_build(&output, tool_config.clone()).await?;

    Ok(())
}

async fn upload_from_args(args: UploadOpts) -> miette::Result<()> {
    if ArchiveType::try_from(&args.package_file).is_none() {
        return Err(miette::miette!(
            "The file {} does not appear to be a conda package.",
            args.package_file.to_string_lossy()
        ));
    }

    let client = AuthenticatedClient::from_client(
        reqwest::Client::builder()
            .no_gzip()
            .build()
            .expect("failed to create client"),
        get_auth_store(args.common.auth_file),
    );

    match args.server_type {
        ServerType::Quetz(quetz_opts) => {
            upload::upload_package_to_quetz(
                &client,
                args.package_file,
                quetz_opts.url,
                quetz_opts.channel,
            )
            .await?;
        }
    }

    Ok(())
}

/// Constructs a default [`EnvFilter`] that is used when the user did not specify a custom RUST_LOG.
pub fn get_default_env_filter(
    verbose: clap_verbosity_flag::LevelFilter,
) -> Result<EnvFilter, ParseError> {
    let mut result = EnvFilter::new("rattler_build=info");

    if verbose >= clap_verbosity_flag::LevelFilter::Trace {
        result = result.add_directive(Directive::from_str("resolvo=info")?);
        result = result.add_directive(Directive::from_str("rattler=info")?);
    } else {
        result = result.add_directive(Directive::from_str("resolvo=warn")?);
        result = result.add_directive(Directive::from_str("rattler=warn")?);
    }

    Ok(result)
}
