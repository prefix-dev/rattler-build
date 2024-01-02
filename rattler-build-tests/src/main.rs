#![deny(dead_code)]

mod tests;
mod utils;

use itertools::Itertools;
use std::{collections::HashSet, path::PathBuf};

use crate::{
    tests::TestFunction,
    utils::{get_target_dir, set_env_without_override, shx},
};

/// entrypoint for all tests
fn main() -> std::io::Result<()> {
    let tests = tests::initialize();
    fn test_data_dir() -> PathBuf {
        PathBuf::from(shx("cargo locate-project --workspace -q --message-format=plain").unwrap())
            .parent()
            .expect("couldn't fetch workspace root")
            .join("test-data")
    }
    let recipes_dir = test_data_dir().join("recipes");

    // build project
    println!("Building rattler-build...");
    shx("cargo build --release -p rattler-build");
    println!("Built rattler-build with release config\n");
    // use binary just built
    let binary = get_target_dir()?.join("release/rattler-build");
    set_env_without_override("RATTLER_BUILD_PATH", binary.to_str().unwrap());

    // cleanup after all tests have successfully completed
    let mut temp_dirs = vec![];
    let (mut successes, mut failures) = (0, 0);

    let (args_set, runs) = std::env::args()
        .skip(1)
        .fold((HashSet::new(), None), |mut acc, x| {
            if x.len() < 3 {
                if let Ok(s) = x.parse::<usize>() {
                    _ = acc.1.insert(s);
                }
            } else {
                acc.0.insert(x);
            }
            acc
        });
    let runs = runs.unwrap_or(4);

    for chunk in &tests
        .into_iter()
        .filter(|(name, _)| {
            args_set.is_empty()
                || args_set.contains(*name)
                || args_set.contains(&name.replace('_', "-"))
        })
        .chunks(runs)
    {
        let mut test_threads = vec![];
        test_threads.reserve(runs);
        for (name, f) in chunk {
            match f {
                TestFunction::NoArg(f) => f(),
                TestFunction::RecipeTemp(f) => {
                    let tmp_dir = std::env::temp_dir().join(name);
                    _ = std::fs::remove_dir_all(&tmp_dir);
                    _ = std::fs::create_dir_all(&tmp_dir);
                    let recipes_dir = recipes_dir.clone();
                    temp_dirs.push(tmp_dir.clone());
                    test_threads
                        .push((name, std::thread::spawn(move || f(&recipes_dir, &tmp_dir))));
                }
            }
        }
        for (name, thread) in test_threads {
            match thread.join() {
                Ok(_) => {
                    println!("Success - rattler-build-tests::test::{name}");
                    successes += 1;
                }
                Err(_) => {
                    println!(
                        "\n\x1B[38;2;255;0;0mFailed\x1B[0m - rattler-build-tests::test::{name}"
                    );
                    failures += 1;
                }
            }
        }
    }

    if failures > 0 {
        println!("\n{} tests failed.", failures);
        println!("{} tests passed.", successes);
    } else {
        println!("\nAll tests({0}/{0}) passed.", successes);
    }
    for tmp_dir in temp_dirs {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    Ok(())
}
