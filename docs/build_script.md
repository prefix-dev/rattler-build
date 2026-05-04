# Scripts for building and testing packages

The `build.sh` file is the build script for Linux and macOS and `build.bat` is
the build script for Windows. These scripts contain the logic that carries out
your build steps. Anything that your build script copies into the `$PREFIX` or
`%PREFIX%` folder will be included in your output package.

For example, this `build.sh`:

```bash title="build.sh"
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

Note: auto-discovery of `build.sh`/`build.bat` only applies to single-output
recipes. In a multi-output recipe, outputs never pick up these files
implicitly; set `build.script` explicitly (either on the individual output or
on the top-level `build:` block) if you want a build script to run.

`build.sh` is run with `bash` and `build.bat` is run with `cmd.exe`.

```yaml title="recipe.yaml"
build:
  script:
    - if: unix
      then:
        - mkdir -p $PREFIX/bin
        - cp $RECIPE_DIR/my_script_with_recipe.sh $PREFIX/bin/super-cool-script.sh
    - if: win
      then:
        - mkdir %LIBRARY_BIN%
        - copy %RECIPE_DIR%\my_script_with_recipe.bat %LIBRARY_BIN%\super-cool-script.bat
```

## Environment variables

There are many environment variables that are automatically set during the build
process.

However, you can also set your own environment variables easily in the `script`
section of your recipe:

```yaml title="recipe.yaml"
build:
  script:
    # Either use `content` or `file` to specify the script
    # Note: this script only works on Unix :)
    content: |
      echo $FOO
      echo $BAR
      echo "Secret value: $BAZ"
    env:
      # hard coded value for `FOO`
      FOO: "foo"
      # Forward a value from the "outer" environment
      # Without `default=...`, the build process will error if `BAR` is not set
      BAR: ${{ env.get("BAR", default="NOBAR") }}
    secrets:
      # This value is a secret and will be masked in the logs and not stored in the rendered recipe
      # The value needs to be available as an environment variable in the outer environment
      - BAZ
```

## Alternative script interpreters

With Rattler-Build and the new recipe syntax you can select an `interpreter`
for your script.

So far, the following interpreters are supported:

- `bash` (default on Unix)
- `cmd.exe` (default on Windows)
- `nushell`
- `python`
- `perl`
- `rscript` (for R scripts)
- `ruby`
- `node` or `nodejs` (for NodeJS scripts)

Rattler-Build automatically detects the interpreter based on the file extension
(`.sh`, `.bat`, `.nu`, `.py`, `.pl`, `.r`, `.rb`, `.js`) or you can specify it in the
`interpreter` key in the `script` section of your recipe.

```yaml title="recipe.yaml"
build:
  script: myscript.py  # automatically selects the Python interpreter

requirements:
  build:
    - python  # required to execute the `myscript.py` script
```

!!! note
    Using alternative interpreters is less battle-tested than using `bash` or
    `cmd.exe`. If you encounter any issues, please
    [open an issue](https://github.com/prefix-dev/rattler-build/issues/new).

### Using `nushell`

In order to use `nushell` you can select the `interpreter: nu` or have a
`build.nu` file in your recipe directory. Nushell works on Windows, macOS and
Linux with the same syntax.

```yaml title="recipe.yaml"
build:
  script:
    interpreter: nu
    content: |
      echo "Hello from nushell!"

# Note: it's required to have `nushell` in the `build` section of your recipe!
requirements:
  build:
    - nushell
```

### Using `python`

In order to use `python` you can select the `interpreter: python` or have a
`build.py` file in your recipe directory and `python` in the
`requirements/build` section.

```yaml title="recipe.yaml"
build:
  script:
    interpreter: python
    content: |
      print("Hello from Python!")

# Note: it's required to have `python` in the `build` section of your recipe!
requirements:
  build:
    - python
```

### Using `ruby`

In order to use `ruby` you can select the `interpreter: ruby` or have a
`build.rb` file in your recipe directory and `ruby` in the
`requirements/build` section.

```yaml title="recipe.yaml"
build:
  script:
    interpreter: ruby
    content: |
      puts "Hello from Ruby!"

# Note: it's required to have `ruby` in the `build` section of your recipe!
requirements:
  build:
    - ruby
```

### Using `nodejs`

In order to use `nodejs` you can select the `interpreter: nodejs` (or `node`) or have a
`build.js` file in your recipe directory and `nodejs` in the
`requirements/build` section.

```yaml title="recipe.yaml"
build:
  script:
    interpreter: nodejs
    content: |
      console.log("Hello from NodeJS!");

# Note: it's required to have `nodejs` in the `build` section of your recipe!
requirements:
  build:
    - nodejs
```


## Default environment variables set during the build process

During the build process, the following environment variables are set, on
Windows with `build.bat` and on macOS and Linux with `build.sh`. By default,
these are the only variables available to your build script. Unless otherwise
noted, no variables are inherited from the shell environment in which you invoke
`rattler-build`. To override this behavior, see :ref:`inherited-env-vars`.

`ARCH`

: Either `32` or `64`, to specify whether the build is 32-bit or 64-bit.
  The value depends on the ARCH environment variable and defaults to the
  architecture the interpreter running conda was compiled with.

`CMAKE_GENERATOR`

: The CMake generator string for the current build environment. On macOS and
  Linux this is always `Unix Makefiles`. Not set on Windows by rattler-build.

`CONDA_BUILD`

: Always set to `1` to indicate that the build process is running. Useful
  for build scripts that need to detect that they are running inside a
  conda-style build.

`CPU_COUNT`

: The number of CPUs on the system, used by build tools that parallelize.
  Falls back to the system's logical CPU count if not set.

`SHLIB_EXT`

: The shared library extension specific to the target platform (e.g. `.so`
  for Linux, `.dylib` for macOS, and `.dll` for Windows).

`HTTP_PROXY`, `HTTPS_PROXY`

: Inherited from the user's shell environment, specifying the HTTP and HTTPS
  proxy settings.

`LANG`

: Defines the system language and locale settings. Set to `C.UTF-8` when
  running with strict environment isolation; otherwise forwarded from the
  user's shell environment.

`LC_ALL`

: Locale category override. Set to `C.UTF-8` when running with strict
  environment isolation; otherwise forwarded from the user's shell environment
  (only forwarded in `conda-build` compatibility mode).

`MAKEFLAGS`

: Forwarded from the user's shell environment in `conda-build` compatibility
  mode. Can be used to set additional arguments for `make`, such as `-j2` to
  utilize 2 CPU cores.

`PY_VER`

: Specifies the Python version against which the build is occurring.
  This can be modified with a `variants.yaml` file.

`PATH`

: Inherited from the user's shell environment and augmented with the
  activated host and build prefixes.

`PREFIX`

: The host prefix into which the build script should install the package's
  files. This is the environment that contains the package's runtime
  dependencies (the `host` requirements). On Windows this is sometimes also
  referred to as `%PREFIX%`. Note that for packages without `host`
  requirements, `PREFIX` and `BUILD_PREFIX` may point at the same environment.

`BUILD_PREFIX`

: The build prefix that contains the tools used to build the package (the
  `build` requirements, such as compilers, `cmake`, `make`, `pkg-config`,
  …). It is a separate environment from `PREFIX` so that build tools do not
  contaminate the runtime dependencies of the package, which is especially
  important when cross-compiling. Use `BUILD_PREFIX` to reference build tools
  (for example `$BUILD_PREFIX/bin/cmake`) and `PREFIX` for files that should
  end up in the final package.

`BUILD_DIR`

: The directory in which the build is executed. This is the parent of
  `SRC_DIR` and contains the work directory as well as auxiliary files such
  as the `pip_cache` directory.

`CONDA_DEFAULT_ENV`

: Set to the host prefix (`PREFIX`) for compatibility with tools that expect
  to find an "active" conda environment.

`CONDA_BUILD_STATE`

: The current phase of the build process. One of `BUILD` (during the build
  script) or `TEST` (during the test phase). Useful in scripts that are
  shared between the build and test phases.

`CONDA_BUILD_CROSS_COMPILATION`

: Set to `1` when the build platform and the target platform differ (i.e.
  the package is being cross-compiled), and `0` otherwise.

`SUBDIR`

: The target subdirectory (platform) for the package being built, e.g.
  `linux-64`, `osx-arm64`, `win-64` or `noarch`.

`build_platform`

: The native subdirectory of the platform that runs the build (e.g.
  `linux-64` on a Linux x86_64 host). This is the platform on which the
  build tools execute.

`host_platform`

: The subdirectory describing the platform that the package's `host`
  dependencies are resolved for. Equal to `target_platform` except for
  `noarch` packages, where `host_platform` is the current build platform.

`target_platform`

: The subdirectory describing the platform that the resulting package will
  run on. Equivalent to `SUBDIR`.

`PKG_BUILDNUM`

: Indicates the build number of the package currently being built.

`PKG_NAME`

: The name of the package that is being built.

`PKG_VERSION`

: The version of the package currently under construction.

`PKG_BUILD_STRING`

: The complete build string of the package being built,
  including the hash (e.g. py311h21422ab_0).

`PKG_HASH`

: Represents the hash of the package being built, excluding the
  leading 'h' (e.g. 21422ab).

`PYTHON`

: The path to the Python executable in the host prefix. Python is
  installed in the host prefix only when it is listed as a host requirement.

`PY3K`

: `1` when the host Python is Python 3, `0` otherwise. Only set when Python
  is part of the host environment.

`R`

: The path to the R executable in the build prefix. R is installed in the
  build prefix only when it is listed as a build requirement.

`R_VER`

: The R version (`major.minor`, e.g. `4.3`). Only set when R is part of the
  variant configuration.

`R_USER`

: The path to the R user library directory inside the host prefix. Only set
  when R is part of the variant configuration.

`NPY_VER`

: The NumPy version (`major.minor`, e.g. `1.26`). Only set when `numpy` is
  part of the variant configuration.

`NPY_DISTUTILS_APPEND_FLAGS`

: Always set to `1` when NumPy is part of the variant configuration. See
  [the conda-build PR](https://github.com/conda/conda-build/pull/3015) for
  background.

`RECIPE_DIR`

: The directory where the recipe is located.

`SP_DIR`

: The location of Python's site-packages, where Python libraries are installed.

`SRC_DIR`

: The path to where the source code is unpacked or cloned. If the
  source file is not a recognized archive format, this directory contains a copy
  of the source file.

`STDLIB_DIR`

: The location of Python's standard library.

`SOURCE_DATE_EPOCH`

: The Unix timestamp (seconds since the epoch) used as a reproducible build
  timestamp. Many build tools honor this variable to produce reproducible
  outputs (see [reproducible-builds.org](https://reproducible-builds.org/docs/source-date-epoch/)).
  rattler-build sets it to the configured build timestamp.

`PYTHONNOUSERSITE`

: Always set to `1` so that the user's site-packages directory is not added
  to `sys.path` during the build, ensuring a clean Python environment.

`PYTHONDONTWRITEBYTECODE`

: Set to `1` for `noarch: python` packages so that no `.pyc` files are
  written during the build.

`PIP_NO_BUILD_ISOLATION`

: Set to `False` so that `pip` does not create its own isolated build
  environment — rattler-build provides the environment instead.

`PIP_NO_DEPENDENCIES`

: Set to `True` so that `pip` does not pull in additional dependencies. All
  dependencies must be specified in the recipe.

`PIP_IGNORE_INSTALLED`

: Set to `True` so that `pip` ignores already-installed packages and
  installs the requested package fresh.

`PIP_NO_INDEX`

: Set to `True` so that `pip` does not query PyPI. All packages must be
  available locally.

`PIP_CACHE_DIR`

: A path inside the work directory used as the `pip` cache for the build.

#### Windows

Unix-style packages on Windows are built in a special `Library` directory under
the build prefix. The environment variables listed in the following table are
defined only on Windows.


| Variable         | Description                                                                                                                  |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `LIBRARY_BIN`    | `<host prefix>\Library\bin`.                                                                                                 |
| `LIBRARY_INC`    | `<host prefix>\Library\include`.                                                                                             |
| `LIBRARY_LIB`    | `<host prefix>\Library\lib`.                                                                                                 |
| `LIBRARY_PREFIX` | `<host prefix>\Library`.                                                                                                     |
| `SCRIPTS`        | `<host prefix>\Scripts`.                                                                                                     |
| `LIB`            | `LIBRARY_LIB` prepended to the inherited `LIB` variable, used by MSVC to find link libraries.                                |
| `INCLUDE`        | `LIBRARY_INC` prepended to the inherited `INCLUDE` variable, used by MSVC to find headers.                                   |
| `CYGWIN_PREFIX`  | The host prefix translated to a Cygwin-style path, such as `/cygdrive/c/path/to/prefix`.                                     |
| `BUILD`          | A target triple of the form `<arch>-pc-windows-<msvc_version>` (e.g. `amd64-pc-windows-19.0.0`). Inherited from env if set.  |

Additionally, on Windows, any environment variables matching the regular
expressions `^VS[0-9]{2,3}COMNTOOLS$` and `^VS[0-9]{4}INSTALLDIR$` are
forwarded so that scripts that rely on locating Visual Studio installations
work as expected.

Not yet supported in Rattler-Build:

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
| `-------------` | T----------------------------------------------------------------------------------------. |
| `VS_VERSION`    | The version number of the Visual Studio version activated within the build, such as `9.0`. |
| `VS_YEAR`       | The release year of the Visual Studio version activated within the build, such as `2008`.  |
-->

### Unix

The environment variables listed in the following table are defined only on
macOS and Linux.

| Variable          | Description                                                                                                                                                                                                            |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `HOME`            | Standard `$HOME` environment variable. In strict isolation mode this is set to the work directory; in `conda-build` compatibility mode it is forwarded from the host shell.                                            |
| `PKG_CONFIG_PATH` | Path to the `pkgconfig` directory, defaults to `$PREFIX/lib/pkgconfig`.                                                                                                                                                |
| `SSL_CERT_FILE`   | Path to the `SSL_CERT_FILE` file (forwarded from the host environment).                                                                                                                                                |
| `CMAKE_GENERATOR` | The CMake generator, set to `Unix Makefiles` on macOS and Linux.                                                                                                                                                       |
| `LC_ALL`          | Locale category override. Set to `C.UTF-8` in strict isolation mode; otherwise forwarded from the host environment.                                                                                                    |
| `CFLAGS`          | Empty, can be forwarded from env to set additional arguments to the C compiler. Only forwarded in `conda-build` compatibility mode.                                                                                    |
| `CXXFLAGS`        | Same as `CFLAGS` for the C++ compiler. Only forwarded in `conda-build` compatibility mode.                                                                                                                             |
| `LDFLAGS`         | Empty, additional flags to be passed to the linker when linking object files into an executable or shared object. Only forwarded in `conda-build` compatibility mode.                                                  |
| `USER`            | Set to `rattler` in strict isolation mode.                                                                                                                                                                             |
| `SHELL`           | Set to `/bin/bash` in strict isolation mode.                                                                                                                                                                           |
| `EDITOR`          | Set to `/bin/false` in strict isolation mode.                                                                                                                                                                          |
| `TERM`            | Set to `xterm-256color` in strict isolation mode.                                                                                                                                                                      |


#### macOS

The environment variables listed in the following table are defined only on
macOS.

| Variable                   | Description                                                                                                              |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `MACOSX_DEPLOYMENT_TARGET` | Same as the Anaconda Python macOS deployment target. Currently `10.9` for intel 32- and 64bit macOS, and 11.0 for arm64. |
| `OSX_ARCH`                 | `i386` or `x86_64` or `arm64`, depending on the target platform                                                          |
| `BUILD`                    | Target triple, e.g. `arm64-apple-darwin20.0.0` for `osx-arm64` or `x86_64-apple-darwin13.4.0` for `osx-64`.              |

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
