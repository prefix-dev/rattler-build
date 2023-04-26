//! This is the main entry point for the `rattler-build` binary.

use anyhow::Ok;
use clap::{arg, Parser};

use indicatif::{MultiProgress, ProgressDrawTarget};
use once_cell::sync::Lazy;
use rattler_conda_types::{NoArchType, Platform};
use selectors::{flatten_selectors, SelectorConfig};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    str::{self, FromStr},
};
use test::TestConfiguration;
use tracing::metadata::LevelFilter;
use tracing_subscriber::{prelude::*, EnvFilter};

mod build;
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
mod unix;
mod used_variables;
mod variant_config;
mod windows;
use build::run_build;

mod test;

use crate::{
    metadata::{BuildConfiguration, Directories, PackageIdentifier},
    render::recipe::render_recipe,
    variant_config::VariantConfig,
};

/// Returns a global instance of [`indicatif::MultiProgress`].
///
/// Although you can always create an instance yourself any logging will interrupt pending
/// progressbars. To fix this issue, logging has been configured in such a way to it will not
/// interfere if you use the [`indicatif::MultiProgress`] returning by this function.
pub fn global_multi_progress() -> MultiProgress {
    static GLOBAL_MP: Lazy<MultiProgress> = Lazy::new(|| {
        let mp = MultiProgress::new();
        mp.set_draw_target(ProgressDrawTarget::stderr_with_hz(20));
        mp
    });
    GLOBAL_MP.clone()
}

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
struct App {
    #[clap(subcommand)]
    subcommand: SubCommands,

    #[clap(short, long, global(true))]
    verbose: bool,
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
}

#[derive(Parser)]
struct TestOpts {
    /// The package file to test
    #[arg(short, long)]
    package_file: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = App::parse();

    let default_filter = if args.verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(default_filter.into())
        .from_env()?
        .add_directive("apple_codesign=off".parse()?);

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .without_time()
        .finish()
        .try_init()
        .unwrap();

    tracing::info!("Starting the build process");

    match args.subcommand {
        SubCommands::Build(args) => run_build_from_args(args).await,
        SubCommands::Test(args) => run_test_from_args(args).await,
    }?;

    Ok(())
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

async fn run_build_from_args(args: BuildOpts) -> anyhow::Result<()> {
    let recipe_path = fs::canonicalize(&args.recipe);
    if let Err(e) = &recipe_path {
        match e.kind() {
            std::io::ErrorKind::NotFound => {
                tracing::error!(
                    "Error: The file {} could not be found.",
                    args.recipe.to_string_lossy()
                );
            }
            std::io::ErrorKind::PermissionDenied => {
                tracing::error!(
                    "Error: Permission denied when trying to access the file {}.",
                    args.recipe.to_string_lossy()
                );
            }
            _ => {
                tracing::error!(
                    "Error: An unknown error occurred while trying to access the file {}: {:?}",
                    args.recipe.to_string_lossy(),
                    e
                );
            }
        }
        std::process::exit(1);
    }
    let mut recipe_path = recipe_path.unwrap();

    // If the recipe_path is a directory, look for "recipe.yaml" in the directory.
    if recipe_path.is_dir() {
        let recipe_yaml_path = recipe_path.join("recipe.yaml");
        if recipe_yaml_path.exists() && recipe_yaml_path.is_file() {
            recipe_path = recipe_yaml_path;
        } else {
            tracing::error!(
                "Error: 'recipe.yaml' not found in the directory {}",
                args.recipe.to_string_lossy()
            );
            std::process::exit(1);
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

    let variant_config = VariantConfig::from_files(&args.variant_config, &selector_config);

    let variants = variant_config
        .find_variants(&recipe_text, &selector_config)
        .expect("Could not compute variants");

    println!("Found variants:");
    for variant in &variants {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Variant", "Version"]);
        for (key, value) in variant.iter() {
            table.add_row(vec![key, value]);
        }
        println!("{}\n", table);
    }

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

        let recipe = render_recipe(&recipe_yaml, &variant, &hash)?;

        if args.render_only {
            println!("{}", serde_yaml::to_string(&recipe).unwrap());
            println!("Variant: {:#?}", variant);
            println!("Hash: {}", recipe.build.string.unwrap());
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
                directories: Directories::create(&name, &recipe_path, &args.output_dir)?,
                channels,
                timestamp: chrono::Utc::now(),
                subpackages,
            },
            finalized_dependencies: None,
        };

        run_build(&output).await?;
    }

    Ok(())
}
