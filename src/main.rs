use anyhow::Result;
// use rattler::MatchSpec;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::collections::HashSet;
use std::path::PathBuf;
// use std::process::Command;
use std::collections::BTreeMap;
use std::str;
use tera::{Context, Tera};
use walkdir::WalkDir;

use tokio;

mod build;
mod metadata;
mod solver;
mod source;
mod hash;
use metadata::{BuildOptions, Metadata, Requirements, Source};

mod packaging;

mod selectors;
use selectors::{eval_selector, flatten_selectors};

use build::run_build;

use crate::source::fetch_sources;

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

fn render_recipe_recursively(recipe: &mut serde_yaml::Mapping, context: &Context) {
    // let mut remove_keys = Vec::new();
    for (k, v) in recipe.iter_mut() {
        // if let YamlValue::String(key) = k {
        //     if let Some(key) = key.strip_prefix("sel(") {
        //         let sel = key.strip_suffix(')').expect("nope");
        //         let esval = eval_selector(sel);
        //         println!("Evaluated {} to {}", sel, esval);
        //         if !esval {
        //             return None;
        //         }
        //         else {
        //             return x
        //         }
        //     }
        // }
        match v {
            YamlValue::String(var) => {
                *v = YamlValue::String(Tera::one_off(var, context, true).unwrap());
            }
            YamlValue::Sequence(var) => {
                render_recipe_recursively_seq(var, context);
            }
            YamlValue::Mapping(var) => {
                render_recipe_recursively(var, context);
            }
            _ => {}
        }
    }
    // remove_keys.iter().for_each(|key| {
    //     recipe.remove(key);
    // });
}

fn render_recipe_recursively_seq(recipe: &mut serde_yaml::Sequence, context: &Context) {
    for v in recipe {
        match v {
            YamlValue::String(var) => {
                *v = YamlValue::String(Tera::one_off(var, context, true).unwrap());
            }
            YamlValue::Sequence(var) => {
                render_recipe_recursively_seq(var, context);
            }
            YamlValue::Mapping(var) => {
                render_recipe_recursively(var, context);
            }
            _ => {}
        }
    }
}

fn render_recipe(recipe: &YamlValue) {
    // Using the tera Context struct
    let recipe = match recipe {
        YamlValue::Mapping(map) => map,
        _ => panic!("Expected a map"),
    };

    let mut context = Context::new();
    if let Some(YamlValue::Mapping(map)) = &recipe.get("context") {
        for (key, v) in map.iter() {
            if let YamlValue::String(key) = key {
                context.insert(key, v);
            }
        }
        let res = Tera::one_off("Name is: {{ name }}", &context, true);
        println!("{}", res.expect("Template no worki"));

        let mut recipe_modified = recipe.clone();
        recipe_modified.remove("context");
        render_recipe_recursively(&mut recipe_modified, &context);
        println!("{:#?}", &recipe_modified);
    } else {
        eprintln!("Did not find context");
    }
}

#[tokio::main]
async fn main() {
    let myrec: YamlValue =
        serde_yaml::from_reader(std::fs::File::open("test.yaml").unwrap()).expect("Give yaml");

    // println!("{:?}", myrec);
    // println!("{}", myrec.name);
    // flatten_selectors(&myrec);
    // render_recipe(&myrec);
    // println!(
    //     "starlark says: {}",
    //     eval_selector("sel(unix and max(4,3) == 4)")
    // );

    // package_conda(Metadata::default());
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
    ).expect("Could not deserialize source");

    print!("{:?}", &sources);

    let output = metadata::Output {
        name: String::from(
            myrec
                .get("name")
                .expect("Could not find name")
                .as_str()
                .expect("..."),
        ),
        version: String::from(
            myrec
                .get("version")
                .expect("Could not find version")
                .as_str()
                .expect("..."),
        ),
        source: sources,
        build: build_options,
        requirements: requirements,
    };

    // let res = create_environment(
    //     vec!["xtensor"],
    //     vec!["conda-forge"],
    //     "/Users/wolfvollprecht/Programs/guessing_game/env".into(),
    // );

    // println!("The result of the micromamba operation is: {:?}", res);
    // fetch_sources(&sources).await;
    run_build(&output).await;

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
