use std::collections::HashSet;
use std::fs::File;
use std::io::Write;

use std::fs;
use std::process::{Command, Stdio};
use std::{io::Read, path::PathBuf};

use rattler_conda_types::Platform;
use rattler_shell::activation::ActivationVariables;
use rattler_shell::{activation::Activator, shell};

use crate::index;
use crate::metadata::{Directories, Output, PlatformOrNoarch};
use crate::os_vars::os_vars;
use crate::packaging::{package_conda, record_files};
use crate::render::resolved_dependencies::resolve_dependencies;
use crate::source::fetch_sources;

macro_rules! s {
    ($x:expr) => {
        String::from($x)
    };
}

pub fn get_build_env_script(output: &Output, directories: &Directories) -> anyhow::Result<PathBuf> {
    let recipe = &output.recipe;
    // TODO revisit this and factor out
    let vars: Vec<(String, String)> = vec![
        (s!("CONDA_BUILD"), s!("1")),
        (s!("PYTHONNOUSERSITE"), s!("1")),
        (
            s!("CONDA_DEFAULT_ENV"),
            s!(directories.host_prefix.to_string_lossy()),
        ),
        // (s!("ARCH"), s!("arm64")),
        (s!("PREFIX"), s!(directories.host_prefix.to_string_lossy())),
        (
            s!("BUILD_PREFIX"),
            s!(directories.build_prefix.to_string_lossy()),
        ),
        (
            s!("SYS_PREFIX"),
            s!(directories.root_prefix.to_string_lossy()),
        ),
        (
            s!("SYS_PYTHON"),
            s!(directories.root_prefix.to_string_lossy()),
        ),
        (
            s!("RECIPE_DIR"),
            s!(directories.recipe_dir.to_string_lossy()),
        ),
        (s!("SRC_DIR"), s!(directories.source_dir.to_string_lossy())),
        (s!("WORK_DIR"), s!(directories.work_dir.to_string_lossy())),
        (s!("BUILD_DIR"), s!(directories.build_dir.to_string_lossy())),
        // pip isolation
        (s!("PIP_NO_BUILD_ISOLATION"), s!("False")),
        (s!("PIP_NO_DEPENDENCIES"), s!("True")),
        (s!("PIP_IGNORE_INSTALLED"), s!("True")),
        (
            s!("PIP_CACHE_DIR"),
            s!(directories
                .work_dir
                .parent()
                .unwrap()
                .join("pip_cache")
                .to_string_lossy()),
        ),
        (s!("PIP_NO_INDEX"), s!("True")),
        (s!("PKG_NAME"), s!(output.name())),
        (s!("PKG_VERSION"), s!(output.version())),
        (s!("PKG_BUILDNUM"), s!(recipe.build.number.to_string())),
        // TODO this is inaccurate
        (
            s!("PKG_BUILD_STRING"),
            s!(recipe.build.string.clone().unwrap_or_default()),
        ),
        (s!("PKG_HASH"), s!(output.build_configuration.hash.clone())),
        // build configuration
        (
            s!("CONDA_BUILD_CROSS_COMPILATION"),
            s!(if output.build_configuration.cross_compilation() {
                "1"
            } else {
                "0"
            }),
        ),
        // (s!("CONDA_BUILD_SYSROOT"), s!("")),
        (
            s!("SUBDIR"),
            s!(output.build_configuration.target_platform.to_string()),
        ),
        (
            s!("build_platform"),
            s!(output.build_configuration.build_platform.to_string()),
        ),
        (
            s!("target_platform"),
            s!(output.build_configuration.target_platform.to_string()),
        ),
        (s!("CONDA_BUILD_STATE"), s!("BUILD")),
        // PY3K
        // "PY_VER": py_ver,
        // "STDLIB_DIR": stdlib_dir,
        // "SP_DIR": sp_dir,
    ];

    let build_env_script_path = directories.work_dir.join("build_env.sh");
    let mut fout = File::create(&build_env_script_path)?;
    for v in vars {
        writeln!(fout, "export {}=\"{}\"", v.0, v.1)?;
    }

    let platform = match output.build_configuration.target_platform {
        PlatformOrNoarch::Platform(p) => p,
        PlatformOrNoarch::Noarch(_) => Platform::NoArch,
    };

    let additional_os_vars = os_vars(&directories.host_prefix, &platform);

    for (k, v) in additional_os_vars {
        writeln!(fout, "export {}=\"{}\"", k, v)?;
    }

    let host_prefix_activator = Activator::from_path(
        &directories.host_prefix,
        shell::Bash,
        output.build_configuration.build_platform,
    )?;
    let current_path = std::env::var("PATH")
        .ok()
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>());

    // if we are in a conda environment, we need to deactivate it before activating the host / build prefix
    let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

    let activation_vars = ActivationVariables {
        conda_prefix,
        path: current_path,
    };

    let host_activation = host_prefix_activator
        .activation(activation_vars)
        .expect("Could not activate host prefix");

    let build_prefix_activator = Activator::from_path(
        &directories.build_prefix,
        shell::Bash,
        output.build_configuration.build_platform,
    )?;

    // We use the previous PATH and _no_ CONDA_PREFIX to stack the build
    // prefix on top of the host prefix
    let activation_vars = ActivationVariables {
        conda_prefix: None,
        path: Some(host_activation.path.clone()),
    };

    let build_activation = build_prefix_activator
        .activation(activation_vars)
        .expect("Could not activate host prefix");

    writeln!(fout, "{}", host_activation.script)?;
    writeln!(fout, "{}", build_activation.script)?;

    Ok(build_env_script_path)
}

pub fn get_conda_build_script(
    output: &Output,
    directories: &Directories,
) -> anyhow::Result<PathBuf> {
    let recipe = &output.recipe;

    let build_env_script_path =
        get_build_env_script(output, directories).expect("Could not write build script");

    let preambel = format!(
        "if [ -z ${{CONDA_BUILD+x}} ]; then\nsource {}\nfi",
        build_env_script_path.to_string_lossy()
    );

    let script = recipe
        .build
        .script
        .clone()
        .unwrap_or_else(|| "build.sh".to_string());
    let script = if script.ends_with(".sh") || script.ends_with(".bat") {
        let recipe_file = directories.recipe_dir.join("build.sh");
        tracing::info!("Reading recipe file: {:?}", recipe_file);

        let mut orig_build_file = File::open(recipe_file).expect("Could not open build.sh file");
        let mut orig_build_file_text = String::new();
        orig_build_file
            .read_to_string(&mut orig_build_file_text)
            .expect("Could not read file");
        orig_build_file_text
    } else {
        script
    };

    let full_script = format!("{}\n{}", preambel, script);
    let build_script_path = directories.work_dir.join("conda_build.sh");

    let mut build_script_file = File::create(&build_script_path)?;
    build_script_file
        .write_all(full_script.as_bytes())
        .expect("Could not write to build script.");

    Ok(build_script_path)
}

pub async fn run_build(output: &Output) -> anyhow::Result<()> {
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

    package_conda(
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

    Ok(())
}
