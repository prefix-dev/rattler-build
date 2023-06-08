//! Functions to collect environment variables that are used during the build process.
use std::path::Path;
use std::{collections::HashMap, env};

use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationError, ActivationVariables, Activator};
use rattler_shell::shell::Shell;

use crate::linux;
use crate::macos;
use crate::metadata::Output;
use crate::unix;
use crate::windows;

/// Returns a map of environment variables that are used in the build process.
/// Also adds platform-specific variables.
pub fn os_vars(prefix: &Path, platform: &Platform) -> HashMap<String, String> {
    let mut vars = HashMap::<String, String>::new();

    vars.insert(
        "CPU_COUNT".to_string(),
        env::var("CPU_COUNT").unwrap_or_else(|_| num_cpus::get().to_string()),
    );
    vars.insert("LANG".to_string(), env::var("LANG").unwrap_or_default());
    vars.insert("LC_ALL".to_string(), env::var("LC_ALL").unwrap_or_default());
    vars.insert(
        "MAKEFLAGS".to_string(),
        env::var("MAKEFLAGS").unwrap_or_default(),
    );

    let shlib_ext = if platform.is_windows() {
        ".dll".to_string()
    } else if platform.is_osx() {
        ".dylib".to_string()
    } else if platform.is_linux() {
        ".so".to_string()
    } else {
        ".not_implemented".to_string()
    };

    vars.insert("SHLIB_EXT".to_string(), shlib_ext);
    vars.insert("PATH".to_string(), env::var("PATH").unwrap_or_default());

    if cfg!(target_family = "windows") {
        vars.extend(windows::env::default_env_vars(prefix, platform).into_iter());
    } else if cfg!(target_family = "unix") {
        vars.extend(unix::env::default_env_vars(prefix).into_iter());
    }

    if platform.is_osx() {
        vars.extend(macos::env::default_env_vars(prefix, platform).into_iter());
    } else if platform.is_linux() {
        vars.extend(linux::env::default_env_vars(prefix).into_iter());
    }

    vars
}

macro_rules! insert {
    ($map:expr, $key:expr, $value:expr) => {
        $map.insert($key.to_string(), $value.to_string());
    };
}

/// Return all variables that should be set during the build process, including
/// operating system specific environment variables.
pub fn vars(output: &Output, build_state: &str) -> HashMap<String, String> {
    let mut vars = HashMap::<String, String>::new();

    insert!(vars, "CONDA_BUILD", "1");
    insert!(vars, "PYTHONNOUSERSITE", "1");

    let directories = &output.build_configuration.directories;
    insert!(
        vars,
        "CONDA_DEFAULT_ENV",
        directories.host_prefix.to_string_lossy()
    );
    insert!(vars, "PREFIX", directories.host_prefix.to_string_lossy());
    insert!(
        vars,
        "BUILD_PREFIX",
        directories.build_prefix.to_string_lossy()
    );
    insert!(vars, "RECIPE_DIR", directories.recipe_dir.to_string_lossy());
    insert!(vars, "SRC_DIR", directories.work_dir.to_string_lossy());
    insert!(vars, "BUILD_DIR", directories.build_dir.to_string_lossy());

    // python variables
    insert!(vars, "PIP_NO_BUILD_ISOLATION", "False");
    insert!(vars, "PIP_NO_DEPENDENCIES", "True");
    insert!(vars, "PIP_IGNORE_INSTALLED", "True");

    let pip_cache = directories.work_dir.parent().unwrap().join("pip_cache");
    insert!(vars, "PIP_CACHE_DIR", pip_cache.to_string_lossy());
    insert!(vars, "PIP_NO_INDEX", "True");

    // pkg vars
    insert!(vars, "PKG_NAME", output.name());
    insert!(vars, "PKG_VERSION", output.version());
    insert!(vars, "PKG_BUILDNUM", output.recipe.build.number.to_string());
    // TODO this is inaccurate
    insert!(
        vars,
        "PKG_BUILD_STRING",
        output.recipe.build.string.clone().unwrap_or_default()
    );
    insert!(vars, "PKG_HASH", output.build_configuration.hash.clone());
    if output.build_configuration.cross_compilation() {
        insert!(vars, "CONDA_BUILD_CROSS_COMPILATION", "1");
    } else {
        insert!(vars, "CONDA_BUILD_CROSS_COMPILATION", "0");
    }
    insert!(
        vars,
        "SUBDIR",
        output.build_configuration.target_platform.to_string()
    );
    insert!(
        vars,
        "build_platform",
        output.build_configuration.build_platform.to_string()
    );
    insert!(
        vars,
        "target_platform",
        output.build_configuration.target_platform.to_string()
    );
    insert!(vars, "CONDA_BUILD_STATE", build_state);

    if let Some(resolved_dependencies) = &output.finalized_dependencies {
        if let Some(host) = &resolved_dependencies.host {
            if let Some(python) = &host
                .resolved
                .iter()
                .find(|d| d.package_record.name == "python")
            {
                let py_version = python.package_record.version.clone();
                if let Some(maj_min) = py_version.as_major_minor() {
                    let py_ver = format!("{}.{}", maj_min.0, maj_min.1);
                    insert!(vars, "PY_VER", py_ver);
                    let site_packages_dir = directories
                        .host_prefix
                        .join(format!("lib/python{}/site-packages", py_ver));
                    insert!(vars, "SP_DIR", site_packages_dir.to_string_lossy());
                }
            }
        }
    }
    // let vars: Vec<(String, String)> = vec![
    //     // (s!("ARCH"), s!("arm64")),
    //     // pip isolation
    //     // build configuration
    //     // (s!("CONDA_BUILD_SYSROOT"), s!("")),
    //     // PY3K
    //     // "PY_VER": py_ver,
    //     // "STDLIB_DIR": stdlib_dir,
    //     // "SP_DIR": sp_dir,
    // ];

    vars
}

#[derive(thiserror::Error, Debug)]
pub enum ScriptError {
    #[error("Failed to write build env script")]
    WriteBuildEnv(#[from] std::io::Error),

    #[error("Failed to write activate script")]
    WriteActivation(#[from] std::fmt::Error),

    #[error("Failed to create activation script")]
    CreateActivation(#[from] ActivationError),
}

/// Write a script that can be sourced to set the environment variables for the build process.
/// The script will also activate the host and build prefixes.
pub fn write_env_script<T: Shell + Clone>(
    output: &Output,
    state: &str,
    out: &mut impl std::io::Write,
    shell_type: T,
) -> Result<(), ScriptError> {
    let directories = &output.build_configuration.directories;

    let vars = vars(output, state);
    let mut s = String::new();
    for v in vars {
        shell_type.set_env_var(&mut s, &v.0, &v.1)?;
    }

    let platform = output.build_configuration.target_platform;

    let additional_os_vars = os_vars(&directories.host_prefix, &platform);

    for (k, v) in additional_os_vars {
        shell_type.set_env_var(&mut s, &k, &v)?;
    }

    writeln!(out, "{}", s)?;

    let host_prefix_activator = Activator::from_path(
        &directories.host_prefix,
        shell_type.clone(),
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
        shell_type,
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

    writeln!(out, "{}", host_activation.script)?;
    writeln!(out, "{}", build_activation.script)?;

    Ok(())
}
