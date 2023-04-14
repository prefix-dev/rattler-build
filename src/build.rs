//! The build module contains the code for running the build process for a given [`Output`]

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;

use std::fs;
use std::process::{Command, Stdio};
use std::{io::Read, path::PathBuf};

use itertools::Itertools;

use crate::env_vars::write_env_script;
use crate::metadata::{Directories, Output};
use crate::packaging::{package_conda, record_files};
use crate::render::resolved_dependencies::resolve_dependencies;
use crate::source::fetch_sources;
use crate::{index, test};

/// Create a conda build script and return the path to it
pub fn get_conda_build_script(
    output: &Output,
    directories: &Directories,
) -> Result<PathBuf, std::io::Error> {
    let recipe = &output.recipe;

    let build_env_script_path = directories.work_dir.join("build_env.sh");
    let mut fout = File::create(&build_env_script_path)?;

    write_env_script(output, "BUILD", &mut fout).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to write build env script: {}", e),
        )
    })?;

    let preambel = format!(
        "if [ -z ${{CONDA_BUILD+x}} ]; then\n    source {}\nfi",
        build_env_script_path.to_string_lossy()
    );

    let default_script = if output.build_configuration.target_platform.is_windows() {
        "build.bat"
    } else {
        "build.sh"
    };

    let script = recipe
        .build
        .script
        .clone()
        .unwrap_or_else(|| vec![default_script.into()])
        .iter()
        .join("\n");

    let script = if script.ends_with(".sh") || script.ends_with(".bat") {
        let recipe_file = directories.recipe_dir.join("build.sh");
        tracing::info!("Reading recipe file: {:?}", recipe_file);

        let mut orig_build_file = File::open(recipe_file)?;
        let mut orig_build_file_text = String::new();
        orig_build_file.read_to_string(&mut orig_build_file_text)?;
        orig_build_file_text
    } else {
        script
    };

    let full_script = format!("{}\n{}", preambel, script);
    let build_script_path = directories.work_dir.join("conda_build.sh");

    let mut build_script_file = File::create(&build_script_path)?;
    build_script_file.write_all(full_script.as_bytes())?;

    Ok(build_script_path)
}

/// Run the build for the given output. This will fetch the sources, resolve the dependencies,
/// and execute the build script. Returns the path to the resulting package.
pub async fn run_build(output: &Output) -> anyhow::Result<PathBuf> {
    let directories = &output.build_configuration.directories;

    if let Some(source) = &output.recipe.source {
        fetch_sources(source, &directories.source_dir, &directories.recipe_dir).await?;
    }

    let finalized_dependencies = resolve_dependencies(output).await?;

    // The output with the resolved dependencies
    let output = Output {
        finalized_dependencies: Some(finalized_dependencies),
        recipe: output.recipe.clone(),
        build_configuration: output.build_configuration.clone(),
    };

    let build_script = get_conda_build_script(&output, directories);
    tracing::info!("Work dir: {:?}", &directories.work_dir);
    tracing::info!("Build script: {:?}", build_script.unwrap());

    let files_before = record_files(&directories.host_prefix).expect("Could not record files");

    Command::new("/bin/bash")
        .current_dir(&directories.source_dir)
        .arg(directories.source_dir.join("conda_build.sh"))
        .stdin(Stdio::null())
        .status()
        .expect("Failed to execute command");

    let files_after = record_files(&directories.host_prefix).expect("Could not record files");

    let difference = files_after
        .difference(&files_before)
        .cloned()
        .collect::<HashSet<_>>();

    let result = package_conda(
        &output,
        &difference,
        &directories.host_prefix,
        &directories.local_channel,
    )?;

    if !output.build_configuration.no_clean {
        fs::remove_dir_all(&directories.build_dir)?;
    }

    index::index(
        &directories.local_channel,
        Some(&output.build_configuration.target_platform),
    )?;

    let test_dir = directories.work_dir.join("test");
    fs::create_dir_all(&test_dir)?;

    tracing::info!("Running tests");
    test::run_test(
        &result,
        Some(&test_dir),
        Some(output.build_configuration.target_platform),
    )
    .await?;

    Ok(result)
}
