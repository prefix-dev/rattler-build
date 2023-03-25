use clap::{arg, Parser};
use render::render_recipe;
use selectors::{flatten_selectors, SelectorConfig};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{collections::BTreeMap, path::PathBuf, str};

mod build;
mod hash;
mod metadata;
mod render;
mod solver;
mod source;
use metadata::{BuildOptions, Requirements, Source};

mod packaging;

mod selectors;
// use selectors::{eval_selector, flatten_selectors};

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
    recipe_file: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Opts::parse();

    let mut myrec: YamlValue =
        serde_yaml::from_reader(std::fs::File::open(args.recipe_file).unwrap()).expect("Give yaml");

    let selector_config = SelectorConfig {
        target_platform: "osx-arm64".to_string(),
        build_platform: "osx-arm64".to_string(),
        python_version: "3.10".to_string(),
    };
    if let Some(flattened_recipe) = flatten_selectors(&mut myrec, &selector_config) {
        println!("Flattened selectors");
        println!("{:#?}", myrec);
        myrec = flattened_recipe;
    } else {
        tracing::error!("Could not flatten selectors");
    }

    let myrec = render_recipe(&myrec);

    print!("{:#?}", myrec);

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

    let sources: Vec<Source> = serde_yaml::from_value(
        myrec
            .get("source")
            .expect("Could not find source key")
            .clone(),
    )
    .expect("Could not deserialize source");

    let about: About = serde_yaml::from_value(
        myrec
            .get("about")
            .expect("Could not find about key")
            .clone(),
    )
    .expect("Could not parse About");

    let output = metadata::Output {
        build: build_options,
        // get package.name
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

    // fetch_sources(&sources).await;
    let res = run_build(&output).await;
    if res.is_err() {
        eprintln!("Build did not succeed");
    }

    // for (k, v) in myrec.context.iter() {
    //     println!("{k}");
    // }

    // // let files = record_files(env::current_dir().unwrap()).expect("Expected files");
    // let files = record_files(PathBuf::from(
    //     "/Users/wolfvollprecht/Programs/guessing_game/src",
    // ))
    // .expect("Expected files");
    // let records = create_paths_json(files).expect("JSON");
    // // println!("{:?}", files);
    // println!("{}", records);

    // let index_json = create_index_json(&myrec).expect("Index json created");
    // println!("{}", index_json);

    // let output = Command::new("/bin/bash")
    //     .arg(myrec.build.script)
    //     .output()
    //     .expect("Failed to execute command");

    // println!(
    //     "{}",
    //     str::from_utf8(output.stdout.as_slice()).expect("give me string")
    // );
}
