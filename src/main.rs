use clap::{arg, Parser};
use render::render_recipe;
use selectors::{flatten_selectors, SelectorConfig};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{collections::BTreeMap, path::PathBuf, str};
use tracing::metadata::LevelFilter;
use tracing_subscriber::{prelude::*, EnvFilter};

mod build;
mod hash;
mod metadata;
mod render;
mod solver;
mod source;
use metadata::{BuildOptions, Requirements, Source};
mod packaging;
mod selectors;
use build::run_build;

use crate::metadata::{About, BuildConfiguration};

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
async fn main() {
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

    let mut myrec: YamlValue =
        serde_yaml::from_reader(std::fs::File::open(&args.recipe_file).unwrap())
            .expect("Give yaml");

    let selector_config = SelectorConfig {
        target_platform: "osx-arm64".to_string(),
        build_platform: "osx-arm64".to_string(),
        python_version: "3.10".to_string(),
    };

    if let Some(flattened_recipe) = flatten_selectors(&mut myrec, &selector_config) {
        myrec = flattened_recipe;
    } else {
        tracing::error!("Could not flatten selectors");
    }

    let myrec = render_recipe(&myrec);

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

    let output = metadata::Output {
        build: build_options,
        name: String::from(
            myrec
                .get("package")
                .expect("Could not find package")
                .get("name")
                .expect("Could not find name")
                .as_str()
                .unwrap(),
        ),
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
            target_platform: String::from("osx-arm64"),
            build_platform: String::from("osx-arm64"),
            hash: String::from("h1234_0"),
            used_vars: vec![],
        },
    };

    let res = run_build(&output, &args.recipe_file).await;
    if res.is_err() {
        eprintln!("Build did not succeed");
    }
}
