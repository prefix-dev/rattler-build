//! Functions to post-process packages after building
//! This includes:
//!
//! - relinking of shared libraries to be relocatable
//! - checking for "overlinking" (i.e. linking to libraries that are not dependencies
//!   of the package, or linking to system libraries that are not part of the allowed list)

use fs_err as fs;
use globset::GlobSet;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use rattler_conda_types::Platform;

use crate::metadata::Output;
use crate::packaging::PackagingError;

pub fn python_bin(prefix: &Path, target_platform: &Platform) -> PathBuf {
    if target_platform.is_windows() {
        prefix.join("python.exe")
    } else {
        prefix.join("bin/python")
    }
}

/// Given a list of files and the path to a Python interpreter, we try to compile any `.py` files
/// to `.pyc` files by invoking the Python interpreter.
pub fn compile_pyc(
    output: &Output,
    paths: &HashSet<PathBuf>,
    base_path: &Path,
    skip_paths: Option<&GlobSet>,
) -> Result<HashSet<PathBuf>, PackagingError> {
    let build_config = &output.build_configuration;
    let python_interpreter = if output.build_configuration.cross_compilation() {
        python_bin(
            &build_config.directories.build_prefix,
            &build_config.build_platform,
        )
    } else {
        python_bin(
            &build_config.directories.host_prefix,
            &build_config.host_platform,
        )
    };

    if !python_interpreter.exists() {
        tracing::debug!(
            "Python interpreter {} does not exist, skipping .pyc compilation",
            python_interpreter.display()
        );
        return Ok(HashSet::new());
    }

    // find the cache tag for this Python interpreter
    let cache_tag = Command::new(&python_interpreter)
        .args(["-c", "import sys; print(sys.implementation.cache_tag)"])
        .output()?
        .stdout;

    let cache_tag = String::from_utf8_lossy(&cache_tag).trim().to_string();

    let file_with_cache_tag = |path: &Path| {
        let mut cache_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        cache_path.push("__pycache__");
        cache_path.push(format!(
            "{}.{}.pyc",
            path.file_stem().unwrap().to_string_lossy(),
            cache_tag
        ));
        cache_path
    };

    // for each file that does not have a corresponding `.pyc` file we try to compile it
    // by invoking Python
    let mut py_files = vec![];
    let mut pyc_files = vec![];
    for entry in paths {
        match entry.extension() {
            Some(e) if e == "pyc" => pyc_files.push(entry.clone()),
            Some(e) if e == "py" => py_files.push(entry.clone()),
            _ => {}
        }
    }

    if let Some(skip_paths) = skip_paths {
        py_files.retain(|p| {
            !skip_paths.is_match(
                p.strip_prefix(base_path)
                    .expect("Should never fail to strip prefix"),
            )
        });
    }

    let mut pyc_files_to_compile = vec![];
    for py_file in py_files {
        if pyc_files.contains(&py_file.with_extension("pyc"))
            || pyc_files.contains(&file_with_cache_tag(&py_file))
        {
            continue;
        }

        pyc_files_to_compile.push(py_file);
    }

    let mut result = HashSet::new();
    if !pyc_files_to_compile.is_empty() {
        tracing::info!("Compiling {} .py files to .pyc", pyc_files_to_compile.len());

        // write files to a temporary file
        let temp_file = tempfile::NamedTempFile::new()?;
        fs::write(
            temp_file.path(),
            pyc_files_to_compile
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n"),
        )?;

        let command = Command::new(&python_interpreter)
            .args(["-m", "compileall", "-i"])
            .arg(temp_file.path())
            .output();

        if command.is_err() {
            let stderr = String::from_utf8_lossy(&command.as_ref().unwrap().stderr);
            tracing::error!("Error compiling .py files to .pyc: {}", stderr);
            return Err(PackagingError::PythonCompileError(stderr.to_string()));
        }

        let command = command.unwrap();
        if !command.status.success() {
            let stderr = String::from_utf8_lossy(&command.stderr);
            tracing::error!("Error compiling .py files to .pyc: {}", stderr);
            return Err(PackagingError::PythonCompileError(stderr.to_string()));
        }

        for file in pyc_files_to_compile {
            let pyc_file = file_with_cache_tag(&file);
            if pyc_file.exists() {
                println!("yes");
                result.insert(pyc_file);
            }
        }
    }

    Ok(result)
}

/// Find any .dist-info/INSTALLER files and replace the contents with "conda"
/// This is to prevent pip from trying to uninstall the package when it is installed with conda
pub fn python(
    output: &Output,
    // TODO maybe introduce a new type to represent the set of paths & prefix
    paths: &HashSet<PathBuf>,
    base_path: &Path,
) -> Result<HashSet<PathBuf>, PackagingError> {
    let name = output.name();
    let version = output.version();
    let mut result = HashSet::new();

    if !output.recipe.build().noarch().is_python() {
        result.extend(compile_pyc(
            output,
            paths,
            base_path,
            output.recipe.build().python().skip_pyc_compilation.globset(),
        )?);
    }

    let metadata_glob = globset::Glob::new("**/*.dist-info/METADATA")?.compile_matcher();

    if let Some(p) = paths.iter().find(|p| metadata_glob.is_match(p)) {
        // unwraps are OK because we already globbed
        let distinfo = p
            .parent()
            .expect("Should never fail to get parent because we already globbed")
            .file_name()
            .expect("Should never fail to get file name because we already globbed")
            .to_string_lossy()
            .to_lowercase();
        if distinfo.starts_with(name.as_normalized())
            && distinfo != format!("{}-{}.dist-info", name.as_normalized(), version)
        {
            tracing::warn!(
                "Found dist-info folder with incorrect name or version: {}",
                distinfo
            );
        }
    }

    let glob = globset::Glob::new("**/*.dist-info/INSTALLER")?.compile_matcher();
    for p in paths {
        if glob.is_match(p) {
            fs::write(p, "conda\n")?;
        }
    }

    Ok(result)
}
