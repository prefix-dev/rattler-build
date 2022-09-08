use super::metadata::Output;
use anyhow;
use std::fs::File;
use std::io::Write;
use std::{env, fs, path};
use std::{
    io::Read,
    os,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn get_work_dir(recipe: &Output) -> anyhow::Result<PathBuf> {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    let dirname = format!("{}_{:?}", recipe.name, since_the_epoch.as_millis());
    let path = env::current_dir()?.join(dirname).join("work");
    println!("Canonicalized: {:?}", path);
    fs::create_dir_all(&path)?;
    Ok(path)
}

macro_rules! s {
    ($x:expr) => {
        String::from($x)
    };
}

pub fn get_build_env_script(build_folder: &PathBuf) -> anyhow::Result<PathBuf> {
    let host_prefix = "...";
    let build_prefix = "...";
    let root_prefix = "...";

    let vars: Vec<(String, String)> = vec![
        (s!("CONDA_BUILD"), s!("1")),
        (s!("PYTHONNOUSERSITE"), s!("1")),
        (s!("CONDA_DEFAULT_ENV"), s!(host_prefix)),
        (s!("ARCH"), s!("arm64")),
        (s!("PREFIX"), s!(host_prefix)),
        (s!("BUILD_PREFIX"), s!(build_prefix)),
        (s!("SYS_PREFIX"), s!(root_prefix)),
        (s!("SYS_PYTHON"), s!(root_prefix)),
    ];

    // export SUBDIR="osx-arm64"
    // export build_platform="osx-arm64"
    // export SRC_DIR="/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/work"
    // export ROOT="/Users/wolfvollprecht/micromamba"
    // export CONDA_PY="39"
    // export PY3K="1"
    // export PY_VER="3.9"
    // export STDLIB_DIR="/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_h_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_pla/lib/python3.9"
    // export SP_DIR="/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_h_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_pla/lib/python3.9/site-packages"
    // export NPY_VER="1.16"
    // export CONDA_NPY="116"
    // export NPY_DISTUTILS_APPEND_FLAGS="1"
    // export PERL_VER="5.26"
    // export CONDA_PERL="5.26"
    // export LUA_VER="5"
    // export CONDA_LUA="5"
    // export R_VER="3.5"
    // export CONDA_R="3.5"
    // export PKG_NAME="libsolv"
    // export PKG_VERSION="0.7.22"
    // export PKG_BUILDNUM="0"
    // export PKG_BUILD_STRING="hd2a9e91_0"
    // export PKG_HASH="hd2a9e91"
    // export RECIPE_DIR="/Users/wolfvollprecht/Programs/boa-forge/libsolv"
    // export CPU_COUNT="8"
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
    // export PIP_NO_BUILD_ISOLATION="False"
    // export PIP_NO_DEPENDENCIES="True"
    // export PIP_IGNORE_INSTALLED="True"
    // export PIP_CACHE_DIR="/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/pip_cache"
    // export PIP_NO_INDEX="True"

    // eval "$('/Users/wolfvollprecht/micromamba/bin/python3.9' -m conda shell.bash hook)"
    // conda activate "/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_h_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_pla"
    // conda activate --stack "/Users/wolfvollprecht/micromamba/conda-bld/libsolv_1657984860857/_build_env"

    let build_env_script_path = build_folder.join("build_env.sh");
    let mut fout = File::create(&build_env_script_path)?;
    for v in vars {
        writeln!(fout, "{}=\"{}\"", v.0, v.1);
    }

    // End of the build env script
    writeln!(fout, "");
    // eval "$('/Users/wolfvollprecht/micromamba/bin/python3.9' -m conda shell.bash hook)"
    // conda activate "$PREFIX"
    // conda activate --stack "$BUILD_PREFIX"

    writeln!(fout, "eval \"$($MAMBA_EXE shell hook)\"");
    writeln!(fout, "micromamba activate \"$PREFIX\"");
    writeln!(fout, "micromamba activate --stack \"$BUILD_PREFIX\"");

    Ok(build_env_script_path)
}

pub fn get_conda_build_script(recipe: &Output, build_folder: &PathBuf) -> anyhow::Result<PathBuf> {
    let build_env_script_path =
        get_build_env_script(build_folder).expect("Could not write build script");
    // let build_env_script_path = build_folder.join("work/build_env.sh");
    let preambel = format!(
        "if [ -z ${{CONDA_BUILD+x}} ]; then\nsource ${{{}}}\nfi",
        build_env_script_path.to_string_lossy()
    );

    // let orig_build_file = File::open(recipe.build.script)
    let mut orig_build_file = File::open("build.sh").expect("Could not open build.sh file");
    let mut orig_build_file_text = String::new();
    orig_build_file
        .read_to_string(&mut orig_build_file_text)
        .expect("Could not read file");
    let full_script = format!("{}\n{}", preambel, orig_build_file_text);
    println!("Full script is {}", full_script);
    let build_script_path = build_folder.join("conda_build.sh");
    println!("Tried to write to {:?}", build_script_path);

    let mut build_script_file = File::create(&build_script_path)?;
    build_script_file
        .write_all(full_script.as_bytes())
        .expect("Could not write to build script.");

    return Ok(build_script_path);
}

pub fn run_build(recipe: &Output) {
    let work_dir = get_work_dir(&recipe).expect("Could not create work directory");
    let build_script = get_conda_build_script(&recipe, &work_dir);
    println!("Work dir: {:?}", work_dir);
    println!("Build script: {:?}", build_script.unwrap());
}
