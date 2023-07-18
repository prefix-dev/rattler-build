//! Functions to collect environment variables that are used during the build process.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{collections::HashMap, env};

use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationError, ActivationVariables, Activator};
use rattler_shell::shell::Shell;

use crate::linux;
use crate::macos;
use crate::metadata::Output;
use crate::unix;
use crate::windows;

fn get_stdlib_dir(prefix: &Path, platform: &Platform, py_ver: &str) -> PathBuf {
    if platform.is_windows() {
        prefix.join("Lib")
    } else {
        let lib_dir = prefix.join("lib");
        lib_dir.join(format!("python{}", py_ver))
    }
}

fn get_sitepackages_dir(prefix: &Path, platform: &Platform, py_ver: &str) -> PathBuf {
    get_stdlib_dir(prefix, platform, py_ver).join("site-packages")
}

/// Returns a map of environment variables for Python that are used in the build process.
///
/// Variables:
/// - PY3K: 1 if Python 3, 0 if Python 2
/// - PY_VER: Python version (major.minor), e.g. 3.8
/// - STDLIB_DIR: Python standard library directory
/// - SP_DIR: Python site-packages directory
/// - NPY_VER: Numpy version (major.minor), e.g. 1.19
/// - NPY_DISTUTILS_APPEND_FLAGS: 1 (https://github.com/conda/conda-build/pull/3015)
pub fn python_vars(
    prefix: &Path,
    platform: &Platform,
    variant: &BTreeMap<String, String>,
) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();

    if let Some(py_ver) = variant.get("python") {
        let py_ver = py_ver.split('.').collect::<Vec<_>>();
        let py_ver_str = format!("{}.{}", py_ver[0], py_ver[1]);
        let stdlib_dir = get_stdlib_dir(prefix, platform, &py_ver_str);
        let site_packages_dir = get_sitepackages_dir(prefix, platform, &py_ver_str);
        result.insert(
            "PY3K".to_string(),
            if py_ver[0] == "3" {
                "1".to_string()
            } else {
                "0".to_string()
            },
        );
        result.insert("PY_VER".to_string(), py_ver_str);
        result.insert(
            "STDLIB_DIR".to_string(),
            stdlib_dir.to_string_lossy().to_string(),
        );
        result.insert(
            "SP_DIR".to_string(),
            site_packages_dir.to_string_lossy().to_string(),
        );
    }

    if let Some(npy_version) = variant.get("numpy") {
        let np_ver = npy_version.split('.').collect::<Vec<_>>();
        let np_ver = format!("{}.{}", np_ver[0], np_ver[1]);

        result.insert("NPY_VER".to_string(), np_ver);
    }
    result.insert("NPY_DISTUTILS_APPEND_FLAGS".to_string(), "1".to_string());

    result
}

/// Returns a map of environment variables for R that are used in the build process.
///
/// Variables:
/// - R_VER: R version (major.minor), e.g. 4.0
/// - R: Path to R executable
/// - R_USER: Path to R user directory
///
pub fn r_vars(
    prefix: &Path,
    platform: &Platform,
    variant: &BTreeMap<String, String>,
) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();

    if let Some(r_ver) = variant.get("r-base") {
        result.insert("R_VER".to_string(), r_ver.clone());

        let r_bin = if platform.is_windows() {
            prefix.join("Scripts/R.exe")
        } else {
            prefix.join("bin/R")
        };

        let r_user = prefix.join("Libs/R");

        result.insert("R".to_string(), r_bin.to_string_lossy().to_string());
        result.insert("R_USER".to_string(), r_user.to_string_lossy().to_string());
    }

    result
}

pub fn language_vars(
    prefix: &Path,
    platform: &Platform,
    variant: &BTreeMap<String, String>,
) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();

    result.extend(python_vars(prefix, platform, variant).into_iter());
    result.extend(r_vars(prefix, platform, variant).into_iter());

    result
}

/// Returns a map of environment variables that are used in the build process.
/// Also adds platform-specific variables.
///
/// Variables:
/// - CPU_COUNT: Number of CPUs
/// - SHLIB_EXT: Shared library extension for platform (e.g. Linux -> .so, Windows -> .dll, macOS -> .dylib)
///
/// Forwards the following environment variables:
/// - PATH: Path where executables are found
/// - LANG: Language (e.g. en_US.UTF-8)
/// - LC_ALL: Language (e.g. en_US.UTF-8)
/// - MAKEFLAGS: Make flags (e.g. -j4)
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

    if let Some((_, host_arch)) = output
        .build_configuration
        .host_platform
        .to_string()
        .rsplit_once('-')
    {
        // TODO clear if we want/need this variable this seems to be pretty bad (in terms of cross compilation, etc.)
        insert!(vars, "ARCH", host_arch);
    }

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
    // hard-code this because we never want pip's build isolation
    // https://github.com/conda/conda-build/pull/2972#discussion_r198290241
    //
    // Note that pip env "NO" variables are inverted logic.
    //    PIP_NO_BUILD_ISOLATION=False means don't use build isolation.
    insert!(vars, "PIP_NO_BUILD_ISOLATION", "False");
    // Some other env vars to have pip ignore dependencies. We supply them ourselves instead.
    insert!(vars, "PIP_NO_DEPENDENCIES", "True");
    insert!(vars, "PIP_IGNORE_INSTALLED", "True");

    // pip's cache directory (PIP_NO_CACHE_DIR) should not be
    // disabled as this results in .egg-info rather than
    // .dist-info directories being created, see gh-3094
    // set PIP_CACHE_DIR to a path in the work dir that does not exist.
    let pip_cache = directories.work_dir.parent().unwrap().join("pip_cache");
    insert!(vars, "PIP_CACHE_DIR", pip_cache.to_string_lossy());
    // tell pip to not get anything from PyPI, please. We have everything we need
    // locally, and if we don't, it's a problem.
    insert!(vars, "PIP_NO_INDEX", "True");

    // For noarch packages, do not write any bytecode
    if output.build_configuration.target_platform == Platform::NoArch {
        insert!(vars, "PYTHONDONTWRITEBYTECODE", "1");
    }

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

    vars.extend(language_vars(
        &directories.host_prefix,
        &output.build_configuration.target_platform,
        &output.build_configuration.variant,
    ));

    // let vars: Vec<(String, String)> = vec![
    //     // build configuration
    //     // (s!("CONDA_BUILD_SYSROOT"), s!("")),
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

    for env_key in &output.recipe.build.script_env.passthrough {
        let var = std::env::var(env_key);
        if let Ok(var) = var {
            shell_type.set_env_var(&mut s, &env_key, &var.as_str())?;
        } else {
            tracing::warn!(
                "Could not find passthrough environment variable: {}",
                env_key
            );
        }
    }

    for (k, v) in &output.recipe.build.script_env.env {
        shell_type.set_env_var(&mut s, &k, &v)?;
    }

    if !output.recipe.build.script_env.secrets.is_empty() {
        tracing::error!("Secrets are not supported yet");
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
        path_modification_behaviour: Default::default(),
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
        path_modification_behaviour: Default::default(),
    };

    let build_activation = build_prefix_activator
        .activation(activation_vars)
        .expect("Could not activate host prefix");

    writeln!(out, "{}", host_activation.script)?;
    writeln!(out, "{}", build_activation.script)?;

    Ok(())
}
