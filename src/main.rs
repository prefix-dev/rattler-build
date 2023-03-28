#![allow(dead_code)]

use clap::{arg, Parser};
use rattler_conda_types::Platform;
use render::render_recipe;
use selectors::{flatten_selectors, SelectorConfig};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{collections::BTreeMap, fs, path::PathBuf, str};
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
use metadata::{BuildOptions, Requirements, Source};
mod index;
mod packaging;
mod selectors;
use build::run_build;

use crate::metadata::{About, BuildConfiguration, Directories};

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

    let mut myrec: YamlValue = serde_yaml::from_reader(std::fs::File::open(&recipe_file).unwrap())
        .expect("Could not parse yaml file");

    let target_platform = if myrec.get("build").and_then(|v| v.get("noarch")).is_some() {
        "noarch".to_string()
    } else {
        Platform::current().to_string()
    };
    tracing::info!("Target platform: {}", target_platform);

    let selector_config = SelectorConfig {
        target_platform: target_platform.clone(),
        build_platform: Platform::current().to_string(),
        python_version: "3.10".to_string(),
    };

    if let Some(flattened_recipe) = flatten_selectors(&mut myrec, &selector_config) {
        myrec = flattened_recipe;
    } else {
        tracing::error!("Could not flatten selectors");
    }

    let myrec = render_recipe(&myrec).expect("Could not render the recipe.");

    let requirements: Requirements = serde_yaml::from_value(
        myrec
            .get("requirements")
            .expect("Could not find key requirements")
            .to_owned(),
    )
    .expect("Could not get requirements");

    let build_options: BuildOptions = serde_yaml::from_value(
        myrec
            .get("build")
            .expect("Could not find build key")
            .clone(),
    )
    .expect("Could not read build options");

    println!("{:#?}", build_options);

    let source_value = myrec.get("source");
    let mut sources: Vec<Source> = Vec::new();
    if let Some(source_value) = source_value {
        if source_value.is_sequence() {
            sources =
                serde_yaml::from_value(source_value.clone()).expect("Could not deserialize source");
        } else {
            sources.push(
                serde_yaml::from_value(source_value.clone()).expect("Could not deserialize source"),
            );
        }
    } else {
        tracing::info!("No sources found");
    }

    let about: About = serde_yaml::from_value(
        myrec
            .get("about")
            .expect("Could not find about key")
            .clone(),
    )
    .expect("Could not parse About");

    let output_name = String::from(
        myrec
            .get("package")
            .expect("Could not find package")
            .get("name")
            .expect("Could not find name")
            .as_str()
            .unwrap(),
    );

    let output = metadata::Output {
        build: build_options,
        name: output_name.clone(),
        version: String::from(
            myrec
                .get("package")
                .expect("Could not find package")
                .get("version")
                .expect("Could not find name")
                .as_str()
                .unwrap(),
        ),
        source: sources,
        requirements,
        about,
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
            directories: Directories::create(&output_name, &recipe_file)?,
        },
    };

    run_build(&output).await
}
