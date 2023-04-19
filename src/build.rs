//! The build module contains the code for running the build process for a given [`Output`]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};

use std::fs;
use std::process::{Command, Stdio};
use std::{io::Read, path::PathBuf};

use itertools::Itertools;
use rattler_shell::shell;

use crate::env_vars::write_env_script;
use crate::metadata::{Directories, Output};
use crate::packaging::{package_conda, record_files};
use crate::render::resolved_dependencies::resolve_dependencies;
use crate::source::fetch_sources;
use crate::test::TestConfiguration;
use crate::{index, test};

/// Create a conda build script and return the path to it
pub fn get_conda_build_script(
    output: &Output,
    directories: &Directories,
) -> Result<PathBuf, std::io::Error> {
    let recipe = &output.recipe;

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
        let recipe_file = directories.recipe_dir.join(script);
        tracing::info!("Reading recipe file: {:?}", recipe_file);

        let mut orig_build_file = File::open(recipe_file)?;
        let mut orig_build_file_text = String::new();
        orig_build_file.read_to_string(&mut orig_build_file_text)?;
        orig_build_file_text
    } else {
        script
    };

    if cfg!(unix) {
        let build_env_script_path = directories.work_dir.join("build_env.sh");
        let preambel = format!(
            "if [ -z ${{CONDA_BUILD+x}} ]; then\n    source {}\nfi",
            build_env_script_path.to_string_lossy()
        );
        let mut fout = File::create(&build_env_script_path)?;
        write_env_script(output, "BUILD", &mut fout, shell::Bash).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to write build env script: {}", e),
            )
        })?;
        let full_script = format!("{}\n{}", preambel, script);
        let build_script_path = directories.work_dir.join("conda_build.sh");

        let mut build_script_file = File::create(&build_script_path)?;
        build_script_file.write_all(full_script.as_bytes())?;
        Ok(build_script_path)
    } else {
        let build_env_script_path = directories.work_dir.join("build_env.bat");
        let preambel = format!(
            "IF \"%CONDA_BUILD%\" == \"\" (\n    call {}\n)",
            build_env_script_path.to_string_lossy()
        );
        let mut fout = File::create(&build_env_script_path)?;

        write_env_script(output, "BUILD", &mut fout, shell::CmdExe).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to write build env script: {}", e),
            )
        })?;

        let full_script = format!("{}\n{}", preambel, script);
        let build_script_path = directories.work_dir.join("conda_build.bat");

        let mut build_script_file = File::create(&build_script_path)?;
        build_script_file.write_all(full_script.as_bytes())?;
        Ok(build_script_path)
    }
}

/// Spawns a process and replaces the given strings in the output with the given replacements.
/// This is used to replace the host prefix with $PREFIX and the build prefix with $BUILD_PREFIX
fn run_process_with_replacements(
    command: &str,
    cwd: &PathBuf,
    args: &[OsString],
    replacements: &[(&str, &str)],
) -> anyhow::Result<()> {
    let mut child = Command::new(command)
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to execute command");

    if let Some(ref mut stdout) = child.stdout {
        let reader = BufReader::new(stdout);

        // Process the output line by line
        for line in reader.lines() {
            if let Ok(line) = line {
                let filtered_line = replacements
                    .iter()
                    .fold(line, |acc, (from, to)| acc.replace(from, to));
                println!("{}", filtered_line);
            } else {
                eprintln!("Error reading output: {:?}", line);
            }
        }
    }

    let status = child.wait().expect("Failed to wait on child");

    if !status.success() {
        return Err(anyhow::anyhow!("Build failed"));
    }

    Ok(())
}

/// Run the build for the given output. This will fetch the sources, resolve the dependencies,
/// and execute the build script. Returns the path to the resulting package.
pub async fn run_build(output: &Output) -> anyhow::Result<PathBuf> {
    let directories = &output.build_configuration.directories;

    index::index(
        &directories.output_dir,
        Some(&output.build_configuration.target_platform),
    )?;

    // Add the local channel to the list of channels
    let mut channels = vec![directories.output_dir.to_string_lossy().to_string()];
    channels.extend(output.build_configuration.channels.clone());

    if let Some(source) = &output.recipe.source {
        fetch_sources(
            source,
            &directories.work_dir,
            &directories.recipe_dir,
            &directories.output_dir,
        )
        .await?;
    }

    let finalized_dependencies = resolve_dependencies(output, &channels).await?;

    // The output with the resolved dependencies
    let output = Output {
        finalized_dependencies: Some(finalized_dependencies),
        recipe: output.recipe.clone(),
        build_configuration: output.build_configuration.clone(),
    };

    let build_script = get_conda_build_script(&output, directories)?;
    tracing::info!("Work dir: {:?}", &directories.work_dir);
    tracing::info!("Build script: {:?}", build_script);

    let files_before = record_files(&directories.host_prefix).expect("Could not record files");

    let (interpreter, args) = if cfg!(unix) {
        ("/bin/bash", vec![build_script.as_os_str().to_owned()])
    } else {
        (
            "cmd.exe",
            vec![
                OsString::from("/d"),
                OsString::from("/c"),
                build_script.as_os_str().to_owned(),
            ],
        )
    };
    run_process_with_replacements(
        interpreter,
        &directories.work_dir,
        &args,
        &[
            (
                directories.host_prefix.to_string_lossy().as_ref(),
                "$PREFIX",
            ),
            (
                directories.build_prefix.to_string_lossy().as_ref(),
                "$BUILD_PREFIX",
            ),
        ],
    )?;

    let files_after = record_files(&directories.host_prefix).expect("Could not record files");

    let difference = files_after
        .difference(&files_before)
        .cloned()
        .collect::<HashSet<_>>();

    let result = package_conda(
        &output,
        &difference,
        &directories.host_prefix,
        &directories.output_dir,
    )?;

    if !output.build_configuration.no_clean {
        fs::remove_dir_all(&directories.build_dir)?;
    }

    index::index(
        &directories.output_dir,
        Some(&output.build_configuration.target_platform),
    )?;

    let test_dir = directories.work_dir.join("test");
    fs::create_dir_all(&test_dir)?;

    println!("{}", output);

    tracing::info!("Running tests");

    test::run_test(
        &result,
        &TestConfiguration {
            test_prefix: test_dir.clone(),
            target_platform: Some(output.build_configuration.target_platform),
            keep_test_prefix: output.build_configuration.no_clean,
            channels,
        },
    )
    .await?;

    if !output.build_configuration.no_clean {
        fs::remove_dir_all(&directories.build_dir)?;
    }

    Ok(result)
}
