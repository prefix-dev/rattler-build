//! Functions to post-process python files after building the package
//!
//! This includes:
//!   - Fixing up the shebangs in scripts
//!   - Compiling `.py` files to `.pyc` files
//!   - Replacing the contents of `.dist-info/INSTALLER` files with "conda"
use fs_err as fs;
use rattler::install::{PythonInfo, get_windows_launcher, python_entry_point_template};
use rattler_conda_types::Platform;
use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::metadata::Output;
use crate::packaging::{PackagingError, TempFiles};
use crate::recipe::parser::GlobVec;
use crate::utils::to_forward_slash_lossy;

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
    skip_paths: &GlobVec,
) -> Result<HashSet<PathBuf>, PackagingError> {
    let build_config = &output.build_configuration;
    let python_interpreter = if output.build_configuration.cross_compilation() {
        python_bin(
            &build_config.directories.build_prefix,
            &build_config.build_platform.platform,
        )
    } else {
        python_bin(
            &build_config.directories.host_prefix,
            &build_config.host_platform.platform,
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

    py_files.retain(|f| {
        // bin files are generally not imported
        if output.build_configuration.target_platform.is_windows() {
            !f.starts_with("Library/bin") && !f.starts_with("Scripts")
        } else {
            !f.starts_with("bin")
        }
    });

    if !skip_paths.is_empty() {
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

        for f in &pyc_files_to_compile {
            let command = Command::new(&python_interpreter)
                .args(["-Wi", "-m", "py_compile"])
                .arg(f)
                .output();

            if command.is_err() {
                let stderr = String::from_utf8_lossy(&command.as_ref().unwrap().stderr);
                tracing::error!("Error compiling .py files to .pyc: {}", stderr);
                return Err(PackagingError::PythonCompileError(stderr.to_string()));
            }

            let command = command.unwrap();
            if !command.status.success() {
                let stderr = String::from_utf8_lossy(&command.stderr);
                tracing::warn!("Error compiling {:?} to .pyc:\n{}", f, stderr);
            }
        }

        for file in pyc_files_to_compile {
            let pyc_file = file_with_cache_tag(&file);
            if pyc_file.exists() {
                result.insert(pyc_file);
            }
        }
    }

    Ok(result)
}

/// Find any .dist-info/INSTALLER files and replace the contents with "conda"
/// This is to prevent pip from trying to uninstall the package when it is installed with conda
pub fn python(temp_files: &TempFiles, output: &Output) -> Result<HashSet<PathBuf>, PackagingError> {
    let name = output.name();
    let version = output.version();
    let mut result = HashSet::new();

    if !output.recipe.build().is_python_version_independent() {
        result.extend(compile_pyc(
            output,
            &temp_files.files,
            temp_files.temp_dir.path(),
            &output.recipe.build().python().skip_pyc_compilation,
        )?);

        // create entry points if it is not a noarch package
        result.extend(create_entry_points(output, temp_files.temp_dir.path())?);
    }

    let metadata_glob = globset::Glob::new("**/*.dist-info/METADATA")?.compile_matcher();

    if let Some(p) = temp_files.files.iter().find(|p| metadata_glob.is_match(p)) {
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
    for p in temp_files.files.iter() {
        if glob.is_match(p) {
            fs::write(p, "conda\n")?;
        }
    }

    Ok(result)
}

fn python_in_prefix(prefix: &Path, use_python_app_entrypoint: bool) -> String {
    if use_python_app_entrypoint {
        format!(
            "/bin/bash {}",
            to_forward_slash_lossy(&prefix.join("bin/pythonw"))
        )
    } else {
        format!("{}", to_forward_slash_lossy(&prefix.join("bin/python")))
    }
}

fn replace_shebang(
    shebang: &str,
    prefix: &Path,
    use_python_app_entrypoint: bool,
) -> (bool, String) {
    // skip first two characters
    let shebang = &shebang[2..];

    let parts = shebang.split_whitespace().collect::<Vec<_>>();

    // split the shebang into its components
    let replaced = parts
        .iter()
        .map(|p| {
            if p.ends_with("/python") || p.ends_with("/pythonw") {
                python_in_prefix(prefix, use_python_app_entrypoint)
            } else {
                p.to_string()
            }
        })
        .collect::<Vec<_>>();

    let modified = parts != replaced;

    (modified, format!("#!{}", replaced.join(" ")))
}

fn fix_shebang(
    path: &Path,
    prefix: &Path,
    use_python_app_entrypoint: bool,
) -> Result<(), io::Error> {
    // make sure that path is a regular file
    if path.is_symlink() || !path.is_file() {
        return Ok(());
    }

    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    // read first line of file
    let mut reader_iter = reader.lines();
    let line = if let Some(l) = reader_iter.next().transpose()? {
        l
    } else {
        return Ok(());
    };

    // check if it starts with #!
    if !line.starts_with("#!") {
        return Ok(());
    }

    let (modified, new_shebang) = replace_shebang(&line, prefix, use_python_app_entrypoint);
    // no modification needed
    if !modified {
        return Ok(());
    }

    let tmp_path = path.with_extension("tmp");
    {
        let file = fs::File::create(&tmp_path)?;
        let mut buf_writer = io::BufWriter::new(file);

        buf_writer.write_all(new_shebang.as_bytes())?;
        buf_writer.write_all(b"\n")?;
        for line in reader_iter {
            buf_writer.write_all(line?.as_bytes())?;
            buf_writer.write_all(b"\n")?;
        }
        buf_writer.flush()?;
    }
    fs::rename(tmp_path, path)?;

    Ok(())
}

/// Create the python entry point script for the recipe. Overwrites any existing entry points.
pub(crate) fn create_entry_points(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    if output.recipe.build().python().entry_points.is_empty() {
        return Ok(Vec::new());
    }

    let mut new_files = Vec::new();

    let (python_record, _) = output.find_resolved_package("python").ok_or_else(|| {
        PackagingError::CannotCreateEntryPoint(
            "Could not find python in host dependencies".to_string(),
        )
    })?;

    // using target_platform is OK because this should never be noarch
    let python_info =
        PythonInfo::from_python_record(&python_record.package_record, *output.target_platform())
            .map_err(|e| {
                PackagingError::CannotCreateEntryPoint(format!(
                    "Could not create python info: {}",
                    e
                ))
            })?;

    for ep in &output.recipe.build().python().entry_points {
        let script = python_entry_point_template(
            &output.prefix().to_string_lossy(),
            output.target_platform().is_windows(),
            ep,
            &python_info,
        );

        if output.target_platform().is_windows() {
            fs::create_dir_all(tmp_dir_path.join("Scripts"))?;

            let script_path = tmp_dir_path.join(format!("Scripts/{}-script.py", ep.command));
            let mut file = fs::File::create(&script_path)?;
            file.write_all(script.as_bytes())?;

            // write exe launcher as well
            let exe_path = tmp_dir_path.join(format!("Scripts/{}.exe", ep.command));
            let mut exe = fs::File::create(&exe_path)?;
            exe.write_all(get_windows_launcher(output.target_platform()))?;

            new_files.extend(vec![script_path, exe_path]);
        } else {
            fs::create_dir_all(tmp_dir_path.join("bin"))?;

            let script_path = tmp_dir_path.join(format!("bin/{}", ep.command));
            let mut file = fs::File::create(&script_path)?;
            file.write_all(script.as_bytes())?;

            #[cfg(target_family = "unix")]
            fs::set_permissions(
                &script_path,
                std::os::unix::fs::PermissionsExt::from_mode(0o775),
            )?;

            if output.target_platform().is_osx()
                && output.recipe.build().python().use_python_app_entrypoint
            {
                fix_shebang(
                    &script_path,
                    output.prefix(),
                    output.recipe.build().python().use_python_app_entrypoint,
                )?;
            }

            new_files.push(script_path);
        }
    }

    Ok(new_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_shebang() {
        let shebang = "#!/some/path/to/python";
        let prefix = PathBuf::from("/Users/runner/miniforge3");
        let new_shebang = replace_shebang(shebang, &prefix, true);

        assert_eq!(
            new_shebang,
            (
                true,
                "#!/bin/bash /Users/runner/miniforge3/bin/pythonw".to_string()
            )
        );

        let new_shebang = replace_shebang(shebang, &prefix, false);
        assert_eq!(
            new_shebang,
            (true, "#!/Users/runner/miniforge3/bin/python".to_string())
        );

        let shebang = "#!/some/path/to/ruby";
        let new_shebang = replace_shebang(shebang, &prefix, false);
        assert_eq!(new_shebang, (false, "#!/some/path/to/ruby".to_string()));
    }

    #[test]
    fn test_replace_shebang_in_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_data_folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/shebang");

        // copy file
        let dest = tempdir.path().join("test.py");
        fs::copy(test_data_folder.join("replace_shebang_1.py"), &dest).unwrap();
        fix_shebang(&dest, &PathBuf::from("/super/prefix"), false).unwrap();
        insta::assert_snapshot!(fs::read_to_string(&dest).unwrap());

        fs::copy(test_data_folder.join("replace_shebang_2.py"), &dest).unwrap();
        fix_shebang(&dest, &PathBuf::from("/super/prefix"), true).unwrap();
        insta::assert_snapshot!(fs::read_to_string(&dest).unwrap());
    }
}
