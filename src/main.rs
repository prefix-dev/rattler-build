//! This is the main entry point for the `rattler-build` binary.

use clap::{arg, crate_version, Parser};

use clap_verbosity_flag::{InfoLevel, Verbosity};
use indicatif::MultiProgress;
use miette::IntoDiagnostic;
use rattler_conda_types::{package::ArchiveType, NoArchType, Platform};
use rattler_networking::AuthenticatedClient;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    str::{self, FromStr},
};
use tracing_subscriber::{filter::Directive, fmt, prelude::*, EnvFilter};

use rattler_build::{
    build::run_build,
    metadata::{BuildConfiguration, Directories, PackageIdentifier},
    recipe::{parser::Recipe, ParsingError},
    selectors::SelectorConfig,
    test::{self, TestConfiguration},
    tool_configuration,
    variant_config::VariantConfig,
};

mod console_utils;
mod hash;

use crate::console_utils::{IndicatifWriter, TracingFormatter};

#[derive(Serialize, Deserialize, Debug)]
struct RawRecipe {
    context: BTreeMap<String, serde_yaml::Value>,
    #[serde(flatten)]
    recipe: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Parser)]
enum SubCommands {
    /// Build a package
    Build(BuildOpts),

    /// Test a package
    Test(TestOpts),
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

    /// Output directory for build artifacts. Defaults to `./output`.
    #[clap(long, env = "CONDA_BLD_PATH", default_value = "./output")]
    output_dir: PathBuf,

    /// The package format to use for the build.
    /// Defaults to `.tar.bz2`.
    #[arg(long, default_value = "tar-bz2")]
    package_format: PackageFormat,
}

#[derive(Parser)]
struct TestOpts {
    /// The package file to test
    #[arg(short, long)]
    package_file: PathBuf,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = App::parse();

    let multi_progress = MultiProgress::new();

    // Setup tracing subscriber
    tracing_subscriber::registry()
        .with(get_default_env_filter(args.verbose.log_level_filter()))
        .with(
            fmt::layer()
                .with_writer(IndicatifWriter::new(multi_progress.clone()))
                .event_format(TracingFormatter),
        )
        .init();

    tracing::info!("Starting the build process");

    match args.subcommand {
        SubCommands::Build(args) => run_build_from_args(args, multi_progress).await,
        SubCommands::Test(args) => run_test_from_args(args).await,
    }
}

async fn run_test_from_args(args: TestOpts) -> miette::Result<()> {
    let package_file = fs::canonicalize(args.package_file).into_diagnostic()?;
    let test_options = TestConfiguration {
        test_prefix: fs::canonicalize(PathBuf::from("test-prefix")).into_diagnostic()?,
        target_platform: Some(Platform::current()),
        keep_test_prefix: false,
        channels: vec!["conda-forge".to_string(), "./output".to_string()],
    };
    test::run_test(&package_file, &test_options)
        .await
        .into_diagnostic()?;
    Ok(())
}

async fn run_build_from_args(args: BuildOpts, multi_progress: MultiProgress) -> miette::Result<()> {
    let recipe_path = fs::canonicalize(&args.recipe);
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
    let mut recipe_path = recipe_path.unwrap();

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

    let recipe_text = fs::read_to_string(&recipe_path).into_diagnostic()?;

    let recipe_yaml: YamlValue =
        serde_yaml::from_str(&recipe_text).expect("Could not parse yaml file");

    // get recipe.build.noarch value as NoArchType from serde_yaml
    let noarch = recipe_yaml
        .get("build")
        .and_then(|v| v.get("noarch"))
        .map(|v| serde_yaml::from_value::<NoArchType>(v.clone()))
        .transpose()
        .into_diagnostic()?
        .unwrap_or_else(NoArchType::none);

    let target_platform = if !noarch.is_none() {
        Platform::NoArch
    } else if let Some(target_platform) = args.target_platform {
        Platform::from_str(&target_platform).into_diagnostic()?
    } else {
        tracing::info!("No target platform specified, using current platform");
        Platform::current()
    };

    let selector_config = SelectorConfig {
        target_platform,
        hash: None,
        build_platform: Platform::current(),
        variant: BTreeMap::new(),
    };

    let variant_config =
        VariantConfig::from_files(&args.variant_config, &selector_config).into_diagnostic()?;

    let outputs_and_variants = variant_config.find_variants(&recipe_text, &selector_config)?;

    tracing::info!("Found variants:");
    for (output, variants) in &outputs_and_variants {
        let package_name = output
            .as_mapping()
            .and_then(|m| m.get("package"))
            .and_then(|v| v.as_mapping())
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_scalar())
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        for variant in variants {
            let mut table = comfy_table::Table::new();
            table
                .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
                .set_header(vec!["Package", "Variant", "Version"]);
            for (key, value) in variant.iter() {
                table.add_row(vec![package_name, key, value]);
            }
            tracing::info!("{}\n", table);
        }
    }

    let tool_config = tool_configuration::Configuration {
        client: AuthenticatedClient::default(),
        multi_progress_indicator: multi_progress,
    };

    for (output, variants) in outputs_and_variants {
        for variant in variants {
            let hash = hash::compute_buildstring(&variant, &noarch);

            let selector_config = SelectorConfig {
                variant: variant.clone(),
                hash: Some(hash.clone()),
                target_platform: selector_config.target_platform,
                build_platform: selector_config.build_platform,
            };

            let recipe = Recipe::from_node(&output, selector_config)
                .map_err(|err| ParsingError::from_partial(&recipe_text, err))?;

            if args.render_only {
                tracing::info!("{}", serde_yaml::to_string(&recipe).unwrap());
                tracing::info!("Variant: {:#?}", variant);
                tracing::info!("Hash: {}", recipe.build().string().unwrap());
                tracing::info!("Skip?: {}", recipe.build().skip());
                continue;
            }

            if recipe.build().skip() {
                tracing::info!("Skipping build for variant: {:#?}", variant);
                continue;
            }

            let mut subpackages = BTreeMap::new();
            subpackages.insert(
                recipe.package().name().clone(),
                PackageIdentifier {
                    name: recipe.package().name().clone(),
                    version: recipe.package().version().to_owned(),
                    build_string: recipe.build().string().unwrap().to_owned(),
                },
            );

            let noarch_type = *recipe.build().noarch();
            let name = recipe.package().name().clone();
            // Add the channels from the args and by default always conda-forge
            let channels = args
                .channel
                .clone()
                .unwrap_or(vec!["conda-forge".to_string()]);

            let output = rattler_build::metadata::Output {
                recipe,
                build_configuration: BuildConfiguration {
                    target_platform,
                    host_platform: match target_platform {
                        Platform::NoArch => Platform::current(),
                        _ => target_platform,
                    },
                    build_platform: Platform::current(),
                    hash: hash::compute_buildstring(&variant, &noarch_type),
                    variant: variant.clone(),
                    no_clean: args.keep_build,
                    directories: Directories::create(
                        name.as_normalized(),
                        &recipe_path,
                        &args.output_dir,
                    )
                    .into_diagnostic()?,
                    channels,
                    timestamp: chrono::Utc::now(),
                    subpackages,
                    package_format: match args.package_format {
                        PackageFormat::TarBz2 => ArchiveType::TarBz2,
                        PackageFormat::Conda => ArchiveType::Conda,
                    },
                },
                finalized_dependencies: None,
            };

            run_build(&output, tool_config.clone()).await?;
        }
    }

    Ok(())
}

/// Constructs a default [`EnvFilter`] that is used when the user did not specify a custom RUST_LOG.
pub fn get_default_env_filter(verbose: clap_verbosity_flag::LevelFilter) -> EnvFilter {
    let mut result = EnvFilter::new("rattler_build=info");

    if verbose >= clap_verbosity_flag::LevelFilter::Trace {
        result = result.add_directive(Directive::from_str("resolvo=info").unwrap());
        result = result.add_directive(Directive::from_str("rattler=info").unwrap());
    } else {
        result = result.add_directive(Directive::from_str("resolvo=warn").unwrap());
        result = result.add_directive(Directive::from_str("rattler=warn").unwrap());
    }

    result
}
