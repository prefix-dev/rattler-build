# Build scripts

The `build.sh` file is the build script for Linux and macOS and `build.bat` is
the build script for Windows. These scripts contain the logic that carries out
your build steps. Anything that your build script copies into the `$PREFIX` or
`%PREFIX%` folder will be included in your output package.

For example, this `build.sh`:

```bash
mkdir -p $PREFIX/bin
cp $RECIPE_DIR/my_script_with_recipe.sh $PREFIX/bin/super-cool-script.sh
```

There are many environment variables defined for you to use in build.sh and
build.bat. Please see [environment variables](#environment-variables) for more
information.

`build.sh` and `build.bat` are optional. You can instead use the `build/script`
key in your `recipe.yaml`, with each value being either a string command or a
list of string commands. Any commands you put there must be able to run on every
platform for which you build. For example, you can't use the `cp` command
because `cmd.exe` won't understand it on Windows.

`build.sh` is run with `bash` and `build.bat` is run with `cmd.exe`.

## Environment variables

### Environment variables set during the build process

During the build process, the following environment variables are set, on
Windows with `build.bat` and on macOS and Linux with `build.sh`. By default,
these are the only variables available to your build script. Unless otherwise
noted, no variables are inherited from the shell environment in which you invoke
`conda-build`. To override this behavior, see :ref:`inherited-env-vars`.

| Variable           | Description                                                                                                                                                                                                                                                        |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ARCH`             | Either `32` or `64`, to specify whether the build is 32-bit or 64-bit. The value depends on the ARCH environment variable and defaults to the architecture the interpreter running conda was compiled with.                                                        |
| `CMAKE_GENERATOR`  | The CMake generator string for the current build environment. On Linux systems, this is always `Unix Makefiles`. On Windows, it is generated according to the Visual Studio version activated at build time, for example, `Visual Studio 9 2008 Win64`.            |
| `CONDA_BUILD=1`    | Always set.                                                                                                                                                                                                                                                        |
| `CPU_COUNT`        | The number of CPUs on the system, as reported by `multiprocessing.cpu_count()`.                                                                                                                                                                                    |
| `SHLIB_EXT`        | The shared library extension.                                                                                                                                                                                                                                      |
| `DIRTY`            | Set to 1 if the `--dirty` flag is passed to the `conda-build` command. May be used to skip parts of a build script conditionally for faster iteration time when developing recipes. For example, downloads, extraction and other things that need not be repeated. |
| `HTTP_PROXY`       | Inherited from your shell environment.                                                                                                                                                                                                                             |
| `HTTPS_PROXY`      | Inherited from your shell environment.                                                                                                                                                                                                                             |
| `LANG`             | Inherited from your shell environment.                                                                                                                                                                                                                             |
| `MAKEFLAGS`        | Inherited from your shell environment. May be used to set additional arguments to make, such as `-j2`, which uses 2 CPU cores to build your recipe.                                                                                                                |
| `PY_VER`           | Python version building against. Set with the `--python` argument or with the CONDA_PY environment variable.                                                                                                                                                       |
| `PATH`             | Inherited from your shell environment and augmented with `$PREFIX/bin`.                                                                                                                                                                                            |
| `PREFIX`           | Build prefix to which the build script should install.                                                                                                                                                                                                             |
| `PKG_BUILDNUM`     | Build number of the package being built.                                                                                                                                                                                                                           |
| `PKG_NAME`         | Name of the package being built.                                                                                                                                                                                                                                   |
| `PKG_VERSION`      | Version of the package being built.                                                                                                                                                                                                                                |
| `PKG_BUILD_STRING` | Complete build string of the package being built, including hash. EXAMPLE: py27h21422ab_0. Conda-build 3.0+.                                                                                                                                                       |
| `PKG_HASH`         | Hash of the package being built, without leading h. EXAMPLE: 21422ab. Conda-build 3.0+.                                                                                                                                                                            |
| `PYTHON`           | Path to the Python executable in the host prefix. Python is installed only in the host prefix when it is listed as a host requirement.                                                                                                                             |
| `R`                | Path to the R executable in the build prefix. R is only installed in the build prefix when it is listed as a build requirement.                                                                                                                                    |
| `RECIPE_DIR`       | Directory of the recipe.                                                                                                                                                                                                                                           |
| `SP_DIR`           | Python's site-packages location.                                                                                                                                                                                                                                   |
| `SRC_DIR`          | Path to where source is unpacked or cloned. If the source file is not a recognized file type---zip, tar, tar.bz2, or tar.xz---this is a directory containing a copy of the source file.                                                                            |
| `STDLIB_DIR`       | Python standard library location.                                                                                                                                                                                                                                  |
| `build_platform`   | The native subdir of the conda executable                                                                                                                                                                                                                          |

Removed from `conda-build` are:
- `NPY_VER`
- `PY3K`

#### Windows

Unix-style packages on Windows are built in a special `Library` directory under the build
prefix. The environment variables listed in the following table are defined only
on Windows.


| Variable         | Description                       |
| ---------------- | --------------------------------- |
| `LIBRARY_BIN`    | `<build prefix>\Library\bin`.     |
| `LIBRARY_INC`    | `<build prefix>\Library\include`. |
| `LIBRARY_LIB`    | `<build prefix>\Library\lib`.     |
| `LIBRARY_PREFIX` | `<build prefix>\Library`.         |
| `SCRIPTS`        | `<build prefix>\Scripts`.         |

Not yet supported in `rattler-build`:

- `CYGWIN_PREFIX`
- `VS_MAJOR`
- `VS_VERSION`
- `VS_YEAR`

Additionally, the following variables are forwarded from the environment:

- `ALLUSERSPROFILE`
- `APPDATA`
- `CommonProgramFiles`
- `CommonProgramFiles(x86)`
- `CommonProgramW6432`
- `COMPUTERNAME`
- `ComSpec`
- `HOMEDRIVE`
- `HOMEPATH`
- `LOCALAPPDATA`
- `LOGONSERVER`
- `NUMBER_OF_PROCESSORS`
- `PATHEXT`
- `ProgramData`
- `ProgramFiles`
- `ProgramFiles(x86)`
- `ProgramW6432`
- `PROMPT`
- `PSModulePath`
- `PUBLIC`
- `SystemDrive`
- `SystemRoot`
- `TEMP`
- `TMP`
- `USERDOMAIN`
- `USERNAME`
- `USERPROFILE`
- `windir`
- `PROCESSOR_ARCHITEW6432`
- `PROCESSOR_ARCHITECTURE`
- `PROCESSOR_IDENTIFIER`


<!--
| `CYGWIN_PREFIX` | Same as PREFIX, but as a Unix-style path, such as `/cygdrive/c/path/to/prefix`.            |
| `-------------` | T----------------------------------------------------------------------------------------. |
| `VS_VERSION`    | The version number of the Visual Studio version activated within the build, such as `9.0`. |
| `VS_YEAR`       | The release year of the Visual Studio version activated within the build, such as `2008`.  |
-->

### Unix

The environment variables listed in the following table are defined only on
macOS and Linux.

| Variable          | Description                                                                                                       |
| ----------------- | ----------------------------------------------------------------------------------------------------------------- |
| `HOME`            | Standard $HOME environment variable.                                                                              |
| `PKG_CONFIG_PATH` | Path to `pkgconfig` directory, defaults to `$PREFIX/lib/pkgconfig                                                 |
| `SSL_CERT_FILE`   | Path to `SSL_CERT_FILE` file.                                                                                     |
| `CFLAGS`          | Empty, can be forwarded from env to set additional arguments to C compiler.                                       |
| `CXXFLAGS`        | Same as CFLAGS for C++ compiler.                                                                                  |
| `LDFLAGS`         | Empty, additional flags to be passed to the linker when linking object files into an executable or shared object. |


#### macOS

The environment variables listed in the following table are defined only on
macOS.

| Variable                   | Description                                                                                                              |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `MACOSX_DEPLOYMENT_TARGET` | Same as the Anaconda Python macOS deployment target. Currently `10.9` for intel 32- and 64bit macOS, and 11.0 for arm64. |
| `OSX_ARCH`                 | `i386` or `x86_64` or `arm64`, depending on the target platform                                                          |

#### Linux

The environment variable listed in the following table is defined only on Linux.

| Variable         | Description                                                                                                                    |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `LD_RUN_PATH`    | Defaults to `<build prefix>/lib`.                                                                                              |
| `QEMU_LD_PREFIX` | The prefix used by QEMU's user mode emulation for library paths.                                                               |
| `QEMU_UNAME`     | Set qemu uname release string to 'uname'.                                                                                      |
| `DEJAGNU`        | The path to the dejagnu testing framework used by the GCC test suite.                                                          |
| `DISPLAY`        | The X11 display to use for graphical applications.                                                                             |
| `BUILD`          | Target triple (`{build_arch}-conda_{build_distro}-linux-gnu`) where build_distro is one of `cos6` or `cos7`, for Centos 6 or 7 |


<!--

## Dynamic behavior based on state of build process

There are times when you may want to process a single file in different ways at
more than 1 step in the render-build-test flow of conda-build. Conda-build sets
the CONDA_BUILD_STATE environment variable during each of these phases. The
possible values are:

* `RENDER`: Set during evaluation of the `recipe.yaml` file.

* `BUILD`: Set during processing of the `build.bat` or `build.sh` script
  files.

* `TEST`: Set during the running of any `run_test` scripts, which also
  includes any commands defined in `meta.yaml` in the `test/commands`
  section.

The CONDA_BUILD_STATE variable is undefined outside of these locations.

-->

<!--
### Git environment variables

The environment variables listed in the following table are defined when the
source is a git repository, specifying the source either with git_url or path.

   * - GIT_BUILD_STR
     - String that joins GIT_DESCRIBE_NUMBER and GIT_DESCRIBE_HASH by an
       underscore.
   * - GIT_DESCRIBE_HASH
     - The current commit short-hash as displayed from `git describe --tags`.
   * - GIT_DESCRIBE_NUMBER
     - String denoting the number of commits since the most recent tag.
   * - GIT_DESCRIBE_TAG
     - String denoting the most recent tag from the current commit, based on the
       output of `git describe --tags`.
   * - GIT_FULL_HASH
     - String with the full SHA1 of the current HEAD.

These can be used in conjunction with templated `meta.yaml` files to set
things---such as the build string---based on the state of the git repository.
-->

<!--
Mercurial environment variables
===============================

The environment variables listed in the following table are defined when the
source is a mercurial repository.

   * - HG_BRANCH
     - String denoting the presently active branch.
   * - HG_BUILD_STR
     - String that joins HG_NUM_ID and HG_SHORT_ID by an underscore.
   * - HG_LATEST_TAG
     - String denoting the most recent tag from the current commit.
   * - HG_LATEST_TAG_DISTANCE
     - String denoting number of commits since the most recent tag.
   * - HG_NUM_ID
     - String denoting the revision number.
   * - HG_SHORT_ID
     - String denoting the hash of the commit.
-->

<!--

Inherited environment variables
===============================

Other than those mentioned above, no variables are inherited from the
environment in which you invoke conda-build. You can choose to inherit
additional environment variables by adding them to `recipe.yaml`:

.. code-block:: yaml

     build:
       script_env:
        - TMPDIR
        - LD_LIBRARY_PATH # [linux]
        - DYLD_LIBRARY_PATH # [osx]

If an inherited variable is missing from your shell environment, it remains
unassigned, but a warning is issued noting that it has no value assigned.

Additionally, values can be set by including `=` followed by the desired
value:

.. code-block:: yaml

     build:
       script_env:
        - MY_VAR=some value

.. warning:: Inheriting environment variables can make it difficult for others
   to reproduce binaries from source with your recipe. Use this feature with
   caution or explicitly set values using the `=` syntax.

.. note:: If you split your build and test phases with `--no-test` and
   `--test`, you need to ensure that the environment variables present at
   build time and test time match. If you do not, the package hashes may use
   different values and your package may not be testable because the hashes will
   differ.

-->
