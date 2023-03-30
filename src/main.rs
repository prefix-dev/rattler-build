#![allow(dead_code)]

use anyhow::Ok;
use clap::{arg, Parser};

use rattler_conda_types::Platform;
use render::render_recipe;
use selectors::{flatten_selectors, SelectorConfig};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{collections::BTreeMap, fs, path::PathBuf, process::exit, str};
use tracing::metadata::LevelFilter;
use tracing_subscriber::{prelude::*, EnvFilter};

mod build;
mod linux;
mod metadata;
mod osx;
mod render;
mod solver;
mod source;
mod unix;
use metadata::{BuildOptions, Requirements};
mod index;
mod packaging;
mod selectors;
mod used_variables;
mod variant_config;
use build::run_build;

use crate::{
    metadata::{BuildConfiguration, Directories, RenderedRecipe},
    used_variables::find_variants,
};

#[derive(Serialize, Deserialize, Debug)]
struct RawRecipe {
    context: BTreeMap<String, serde_yaml::Value>,
    #[serde(flatten)]
    recipe: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Output {
    name: String,
    version: String,
    #[serde(default)]
    build: BuildOptions,
    #[serde(default)]
    requirements: Requirements,
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
        .from_env()
        .unwrap();

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

    // let used_vars = used_variables::used_vars_from_jinja(&recipe_text);

    let variant_config = variant_config::load_variant_configs(&args.variant_config);
    print!("Variant config: {:#?}", variant_config);

    let mut myrec: YamlValue =
        serde_yaml::from_str(&recipe_text).expect("Could not parse yaml file");

    let target_platform = if myrec.get("build").and_then(|v| v.get("noarch")).is_some() {
        "noarch".to_string()
    } else if let Some(target_platform) = args.target_platform {
        target_platform
    } else {
        tracing::info!("No target platform specified, using current platform");
        Platform::current().to_string()
    };

    let variants = find_variants(&recipe_text, &variant_config, &target_platform);

    tracing::info!("Target platform: {}", target_platform);

    let selector_config = SelectorConfig {
        target_platform: target_platform.clone(),
        build_platform: Platform::current().to_string(),
        variant: BTreeMap::new(), // python_version: "3.10".to_string(),
    };

    if let Some(flattened_recipe) = flatten_selectors(&mut myrec, &selector_config) {
        myrec = flattened_recipe;
    } else {
        tracing::error!("Could not flatten selectors");
    }

    if args.render_only {
        for variant in variants {
            let myrec = render_recipe(&myrec, variant).expect("Could not render the recipe.");
            println!("{}", serde_yaml::to_string(&myrec).unwrap());
        }
        exit(0);
    }

    for variant in variants {
        let recipe: serde_yaml::Mapping =
            render_recipe(&myrec, variant).expect("Could not render the recipe.");

        let recipe: RenderedRecipe = serde_yaml::from_value(YamlValue::from(recipe))
            .expect("Could not parse into rendered recipe");

        let name = recipe.package.name.clone();
        let output = metadata::Output {
            recipe,
            build_configuration: BuildConfiguration {
                target_platform: target_platform.clone(),
                host_platform: if target_platform == "noarch" {
                    Platform::current().to_string()
                } else {
                    target_platform.clone()
                },
                build_platform: Platform::current().to_string(),
                hash: String::from("h1234_0"),
                used_vars: vec![],
                no_clean: true,
                directories: Directories::create(&name, &recipe_file)?,
            },
        };

        tracing::info!("{:?}", output);

        run_build(&output).await?;
    }

    Ok(())
}
