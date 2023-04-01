use std::collections::HashSet;
use std::fs::File;
use std::io::Write;

use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{env, fs};
use std::{io::Read, path::PathBuf};

use rattler_conda_types::MatchSpec;

use crate::metadata::{Directories, Output};
use crate::packaging::{package_conda, record_files};
use crate::source::fetch_sources;
use crate::{index, solver};

macro_rules! s {
    ($x:expr) => {
        String::from($x)
    };
}

pub fn get_build_env_script(output: &Output, directories: &Directories) -> anyhow::Result<PathBuf> {
    let recipe = &output.recipe;

    let vars: Vec<(String, String)> = vec![
        (s!("CONDA_BUILD"), s!("1")),
        (s!("PYTHONNOUSERSITE"), s!("1")),
        (
            s!("CONDA_DEFAULT_ENV"),
            s!(directories.host_prefix.to_string_lossy()),
        ),
        (s!("ARCH"), s!("arm64")),
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
        (
            s!("CPU_COUNT"),
            s!(env::var("CPU_COUNT").unwrap_or_else(|_| num_cpus::get().to_string())),
        ),
        // PY3K
        // "PY_VER": py_ver,
        // "STDLIB_DIR": stdlib_dir,
        // "SP_DIR": sp_dir,
    ];

    // export ROOT="/Users/wolfvollprecht/micromamba"
    // export CONDA_PY="39"
    // export NPY_VER="1.16"
    // export CONDA_NPY="116"
    // export NPY_DISTUTILS_APPEND_FLAGS="1"
    // export PERL_VER="5.26"
    // export CONDA_PERL="5.26"
    // export LUA_VER="5"
    // export CONDA_LUA="5"
    // export R_VER="3.5"
    // export CONDA_R="3.5"
    // export SHLIB_EXT=".dylib"
    // export PATH="/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_build_env:/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_build_env/bin:/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_h_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_pla:/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_h_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_pla/bin:/Users/wolfvollprecht/micromamba/bin:/Users/wolfvollprecht/micromamba/condabin:/opt/local/bin:/opt/local/sbin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/share/dotnet:~/.dotnet/tools:/Library/Apple/usr/bin:/Library/Frameworks/Mono.framework/Versions/Current/Commands"
    // export HOME="/Users/wolfvollprecht"
    // export PKG_CONFIG_PATH="/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_h_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_pla/lib/pkgconfig"
    // export CMAKE_GENERATOR="Unix Makefiles"
    // export OSX_ARCH="arm64"
    // export MACOSX_DEPLOYMENT_TARGET="11.0"
    // export BUILD="arm64-apple-darwin20.0.0"
    // export target_platform="osx-arm64"
    // export CONDA_BUILD_SYSROOT="/Applications/Xcode_12.4.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX11.1.sdk"
    // export macos_machine="arm64-apple-darwin20.0.0"
    // export FEATURE_STATIC="0"
    // export CONDA_BUILD_STATE="BUILD"
    // export CLICOLOR_FORCE="1"
    // export AM_COLOR_TESTS="always"
    // export MAKE_TERMOUT="1"
    // export CMAKE_COLOR_MAKEFILE="ON"
    // export CXXFLAGS="-fdiagnostics-color=always"
    // export CFLAGS="-fdiagnostics-color=always"

    let build_env_script_path = directories.work_dir.join("build_env.sh");
    let mut fout = File::create(&build_env_script_path)?;
    for v in vars {
        writeln!(fout, "export {}=\"{}\"", v.0, v.1)?;
    }

    writeln!(
        fout,
        "\nexport MAMBA_EXE={}",
        env::var("MAMBA_EXE").expect("Could not find MAMBA_EXE")
    )?;
    writeln!(fout, "eval \"$($MAMBA_EXE shell hook)\"")?;
    writeln!(fout, "micromamba activate \"$PREFIX\"")?;
    writeln!(fout, "micromamba activate --stack \"$BUILD_PREFIX\"")?;

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

pub async fn setup_environments(output: &Output, directories: &Directories) -> anyhow::Result<()> {
    let recipe = &output.recipe;

    if !recipe.requirements.build.is_empty() {
        let specs = recipe
            .requirements
            .build
            .iter()
            .map(|s| MatchSpec::from_str(s))
            .collect::<Result<Vec<MatchSpec>, _>>()?;

        solver::create_environment(
            specs,
            &output.build_configuration.build_platform,
            &directories.build_prefix,
            vec!["conda-forge".to_string()],
        )
        .await?;
    } else {
        fs::create_dir_all(&directories.build_prefix)?;
    }

    if !recipe.requirements.host.is_empty() {
        let specs = recipe
            .requirements
            .host
            .iter()
            .map(|s| MatchSpec::from_str(s))
            .collect::<Result<Vec<MatchSpec>, _>>()?;

        solver::create_environment(
            specs,
            &output.build_configuration.host_platform,
            &directories.host_prefix,
            vec!["conda-forge".to_string()],
        )
        .await?;
    } else {
        fs::create_dir_all(&directories.host_prefix)?;
    }

    Ok(())
}

pub async fn run_build(output: &Output) -> anyhow::Result<()> {
    let directories = &output.build_configuration.directories;
    let build_script = get_conda_build_script(output, directories);

    tracing::info!("Work dir: {:?}", &directories.work_dir);
    tracing::info!("Build script: {:?}", build_script.unwrap());

    fetch_sources(
        &output.recipe.source,
        &directories.source_dir,
        &directories.recipe_dir,
    )
    .await?;

    setup_environments(output, directories).await?;

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
        output,
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
