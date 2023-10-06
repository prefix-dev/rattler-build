//! This is the main entry point for the `rattler-build` binary.

use anyhow::Ok;
use clap::{arg, crate_version, Parser};

use clap_verbosity_flag::{InfoLevel, Verbosity};
use indicatif::MultiProgress;
use rattler_conda_types::{package::ArchiveType, NoArchType, Platform};
use rattler_networking::AuthenticatedClient;
use selectors::{flatten_selectors, SelectorConfig};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::ExitCode,
    str::{self, FromStr},
};
use test::TestConfiguration;
use tracing_subscriber::{
    filter::Directive,
    fmt,
    prelude::*,
    EnvFilter,
};

mod build;
mod console_utils;
mod env_vars;
mod hash;
mod index;
mod linux;
mod macos;
mod metadata;
mod packaging;
mod post;
mod render;
mod selectors;
mod source;
mod tool_configuration;
mod unix;
mod used_variables;
mod variant_config;
mod windows;
use build::run_build;

mod test;

use crate::{
    console_utils::{IndicatifWriter, TracingFormatter},
    metadata::{BuildConfiguration, Directories, PackageIdentifier},
    render::recipe::render_recipe,
    variant_config::VariantConfig,
};

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
async fn main() -> ExitCode {
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

    let result = match args.subcommand {
        SubCommands::Build(args) => run_build_from_args(args, multi_progress).await,
        SubCommands::Test(args) => run_test_from_args(args).await,
    };

    match result {
        Result::Ok(_) => ExitCode::SUCCESS,
        Result::Err(e) => {
            tracing::error!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

async fn run_test_from_args(args: TestOpts) -> anyhow::Result<()> {
    let package_file = fs::canonicalize(args.package_file)?;
    let test_options = TestConfiguration {
        test_prefix: fs::canonicalize(PathBuf::from("test-prefix"))?,
        target_platform: Some(Platform::current()),
        keep_test_prefix: false,
        channels: vec!["conda-forge".to_string(), "./output".to_string()],
    };
    test::run_test(&package_file, &test_options).await?;
    Ok(())
}

async fn run_build_from_args(args: BuildOpts, multi_progress: MultiProgress) -> anyhow::Result<()> {
    let recipe_path = fs::canonicalize(&args.recipe);
    if let Err(e) = &recipe_path {
        match e.kind() {
            std::io::ErrorKind::NotFound => {
                return Err(anyhow::anyhow!(
                    "The file {} could not be found.",
                    args.recipe.to_string_lossy()
                ));
            }
            std::io::ErrorKind::PermissionDenied => {
                return Err(anyhow::anyhow!(
                    "Permission denied when trying to access the file {}.",
                    args.recipe.to_string_lossy()
                ));
            }
            _ => {
                return Err(anyhow::anyhow!(
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
            return Err(anyhow::anyhow!(
                "'recipe.yaml' not found in the directory {}",
                args.recipe.to_string_lossy()
            ));
        }
    }

    let recipe_text = fs::read_to_string(&recipe_path)?;

    let mut recipe_yaml: YamlValue =
        serde_yaml::from_str(&recipe_text).expect("Could not parse yaml file");

    // get recipe.build.noarch value as NoArchType from serde_yaml
    let noarch = recipe_yaml
        .get("build")
        .and_then(|v| v.get("noarch"))
        .map(|v| serde_yaml::from_value::<NoArchType>(v.clone()))
        .transpose()?
        .unwrap_or_else(NoArchType::none);

    let target_platform = if !noarch.is_none() {
        Platform::NoArch
    } else if let Some(target_platform) = args.target_platform {
        Platform::from_str(&target_platform)?
    } else {
        tracing::info!("No target platform specified, using current platform");
        Platform::current()
    };

    let selector_config = SelectorConfig {
        target_platform,
        build_platform: Platform::current(),
        variant: BTreeMap::new(),
    };

    let variant_config = VariantConfig::from_files(&args.variant_config, &selector_config)?;

    let variants = variant_config
        .find_variants(&recipe_text, &selector_config)
        .expect("Could not compute variants");

    tracing::info!("Found variants:");
    for variant in &variants {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Variant", "Version"]);
        for (key, value) in variant.iter() {
            table.add_row(vec![key, value]);
        }
        tracing::info!("{}\n", table);
    }

    let tool_config = tool_configuration::Configuration {
        client: AuthenticatedClient::default(),
        multi_progress_indicator: multi_progress,
    };

    for variant in variants {
        let hash = hash::compute_buildstring(&variant, &noarch);

        let selector_config = SelectorConfig {
            variant: variant.clone(),
            target_platform: selector_config.target_platform,
            build_platform: selector_config.build_platform,
        };

        if let Some(flattened_recipe) = flatten_selectors(&mut recipe_yaml, &selector_config) {
            recipe_yaml = flattened_recipe;
        } else {
            tracing::error!("Could not flatten selectors");
        }

        let recipe = match render_recipe(&recipe_yaml, &variant, &hash) {
            Result::Err(e) => {
                match &e {
                    render::recipe::RecipeRenderError::InvalidYaml(inner) => {
                        tracing::error!("Failed to parse recipe YAML: {}", inner.to_string());
                    }
                    render::recipe::RecipeRenderError::YamlNotMapping => {
                        tracing::error!("{}", e);
                    }
                }
                return Err(e.into());
            }
            Result::Ok(r) => r,
        };

        if args.render_only {
            tracing::info!("{}", serde_yaml::to_string(&recipe).unwrap());
            tracing::info!("Variant: {:#?}", variant);
            tracing::info!("Hash: {}", recipe.build.string.unwrap());
            continue;
        }

        let mut subpackages = BTreeMap::new();
        subpackages.insert(
            recipe.package.name.clone(),
            PackageIdentifier {
                name: recipe.package.name.clone(),
                version: recipe.package.version.clone(),
                build_string: recipe.build.string.clone().unwrap(),
            },
        );

        let noarch_type = recipe.build.noarch;
        let name = recipe.package.name.clone();
        // Add the channels from the args and by default always conda-forge
        let channels = args
            .channel
            .clone()
            .unwrap_or(vec!["conda-forge".to_string()]);

        let output = metadata::Output {
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
                )?,
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
