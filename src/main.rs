#![allow(dead_code)]

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
    process::exit,
    str::{self, FromStr},
};
use tracing::metadata::LevelFilter;
use tracing_subscriber::{prelude::*, EnvFilter};

mod build;
mod linux;
mod macos;
mod metadata;
mod os_vars;
mod render;
mod source;
mod unix;
mod windows;
use metadata::PlatformOrNoarch;
mod index;
mod packaging;
mod selectors;
mod used_variables;
mod variant_config;
use build::run_build;

use crate::{
    metadata::{BuildConfiguration, Directories},
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
struct Opts {
    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    recipe_file: PathBuf,

    #[arg(long)]
    target_platform: Option<String>,

    #[arg(short = 'm', long)]
    variant_config: Vec<PathBuf>,

    #[arg(long)]
    render_only: bool,

    #[arg(long)]
    keep_build: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Opts::parse();

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

    let recipe_file = fs::canonicalize(args.recipe_file)?;
    let recipe_text = fs::read_to_string(&recipe_file)?;

    let mut recipe_yaml: YamlValue =
        serde_yaml::from_str(&recipe_text).expect("Could not parse yaml file");

    let target_platform: PlatformOrNoarch = if recipe_yaml
        .get("build")
        .and_then(|v| v.get("noarch"))
        .is_some()
    {
        let noarch_type: NoArchType = serde_yaml::from_value(
            recipe_yaml
                .get("build")
                .unwrap()
                .get("noarch")
                .unwrap()
                .clone(),
        )?;
        PlatformOrNoarch::Noarch(noarch_type)
    } else if let Some(target_platform) = args.target_platform {
        PlatformOrNoarch::Platform(Platform::from_str(&target_platform)?)
    } else {
        tracing::info!("No target platform specified, using current platform");
        PlatformOrNoarch::Platform(Platform::current())
    };

    let selector_config = SelectorConfig {
        target_platform: target_platform.clone(),
        build_platform: Platform::current(),
        variant: BTreeMap::new(),
    };

    let variant_config = VariantConfig::from_files(&args.variant_config, &selector_config);
    print!("Variant config: {:#?}", variant_config);

    let variants = variant_config
        .find_variants(&recipe_text, &selector_config)
        .expect("Could not compute variants");

    println!("Variants: {:#?}", variants);

    if let Some(flattened_recipe) = flatten_selectors(&mut recipe_yaml, &selector_config) {
        recipe_yaml = flattened_recipe;
    } else {
        tracing::error!("Could not flatten selectors");
    }

    if args.render_only {
        for variant in variants {
            let rendered_recipe =
                render_recipe(&recipe_yaml, &variant).expect("Could not render the recipe.");
            println!("{}", serde_yaml::to_string(&rendered_recipe).unwrap());
        }
        exit(0);
    }

    for variant in variants {
        let recipe = render_recipe(&recipe_yaml, &variant)?;

        let name = recipe.package.name.clone();
        let output = metadata::Output {
            recipe,
            build_configuration: BuildConfiguration {
                target_platform: target_platform.clone(),
                host_platform: match target_platform {
                    PlatformOrNoarch::Platform(p) => p,
                    PlatformOrNoarch::Noarch(_) => Platform::current(),
                },
                build_platform: Platform::current(),
                hash: String::from("h1234_0"),
                variant: variant.clone(),
                no_clean: args.keep_build,
                directories: Directories::create(&name, &recipe_file)?,
                channels: vec!["conda-forge".to_string()],
            },
            finalized_dependencies: None,
        };

        run_build(&output).await?;
    }

    Ok(())
}
