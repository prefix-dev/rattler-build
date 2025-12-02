# Windows quirks

Building packages on Windows is different from Unix systems (Linux and macOS). There are a few historical quirks, as well as the fact that the MSVC compiler is _not redistributable_ and has to be installed manually on the host system.

## The filesystem layout on Windows

The Windows filesystem layout is a bit different than the standard Unix one. This means where Unix recipes use `$PREFIX`, Windows should usually use `%LIBRARY_PREFIX%` which points to `%PREFIX%\Library\` instead of _just_ `%PREFIX%`.

Note: this is handled automatically when using conda-forge packages for CMake, Meson by using `%CMAKE_ARGS%` or `%MESON_ARGS%` which include the correct values for the installation prefix.

On the top level, there is an additional `Scripts` folder, as well as a `bin/` folder. On Windows (unlike Unix systems), a total of 5 paths are added upon activation:

```bat
%PREFIX%
%PREFIX%\Library\mingw-w64\bin
%PREFIX%\Library\usr\bin
%PREFIX%\Library\bin
%PREFIX%\Scripts
%PREFIX%\bin
```

Additionally, the site-packages folder is _also_ located at the root of the filesystem layout:

```text
%PREFIX%
├── Library
│   ├── lib
│   ├── bin
│   ├── share
│   ├── etc
│   └── ...
├── site-packages
├── Scripts
└── bin
```

The reasons for this layout are historical: Python on Windows traditionally installs packages to `site-packages` at the root, and `Scripts` is where Python console scripts and entry points are placed. The `Library` folder mimics a Unix-style hierarchy for non-Python packages.

To make this easier, certain shortcut env vars are available on Windows: `%LIBRARY_PREFIX%`, `%LIBRARY_BIN%`, `%LIBRARY_INC%` (for `Library\include`), and `%LIBRARY_LIB%`.

## Build scripts

### Cmd.exe

The _default interpreter_ for build scripts on Windows is `cmd.exe` which has a quite clunky syntax and execution model.

It will, for example, skip over errors if you do not manually insert `if %ERRORLEVEL% neq 0 exit 1` after each statement. If the build script is a list of commands, then rattler-build will automatically inject this after each list item. If you pass in a complete build script or file, you will have to do this manually to recognize issues in command execution early on.

### Using Powershell

You can select PowerShell as an interpreter, which comes pre-installed on Windows these days. To do so, just set

```yaml title="recipe.yaml"
build:
  script:
    interpreter: powershell
    script: ...
```

Or save your build script as `build.ps1` (which will automatically use powershell based on the file ending).

### Using `Bash` on Windows

To use bash on Windows, you can install bash in your build requirements (e.g. on conda-forge it would be `m2-bash`) and call the bash script from a cmd.exe script:

```batch title="build.bat"
bash %RECIPE_DIR%/build_win.sh
if %ERRORLEVEL% neq 0 exit 1

...
```

To find more examples of this pattern, [try this Github search query](https://github.com/search?q=org%3Aconda-forge+bash+language%3ABatchfile+&type=code).

## Installing the correct MSVC compilers

In order to install the correct MSVC compilers, you should get the Community [Visual Studio Installer](https://visualstudio.microsoft.com/downloads/).

The `C` / `C++` compiler that is installed on the host system needs to match the requirements of the recipe. For example, if the recipe uses `vs2022`, then you will need `Visual Studio Compilers 2022` installed on the host system. The same goes for `vs2026`, `vs2017`, etc. The installer also allows you to have multiple versions installed simultaneously. The "activation scripts" of the package will automatically _select_ the correct version by setting the environment variables properly.

You can use the GUI to install the right version of Visual Studio Compilers, or you can use the following commands in Powershell:

```powershell
# Install C/C++ build tools with Winget (through community installer)
winget install Microsoft.VisualStudio.BuildTools --silent --accept-source-agreements --accept-package-agreements `
    --override "--passive --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# For VS2019, use --add Microsoft.VisualStudio.ComponentGroup.VC.Tools.142.x86.x64
# For Windows 11 SDK, --add Microsoft.VisualStudio.Component.Windows11SDK.22621
```

You can find more [documentation](https://learn.microsoft.com/en-us/visualstudio/install/command-line-parameter-examples?view=visualstudio#using-winget) and [more identifiers](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=visualstudio#desktop-development-with-c) on the Microsoft Visual Studio documentation.

## MinGW64 compiler stack

!!! note conda-forge specific information
    Everything that follows on this page is specific to `conda-forge` and choices this distribution made. If you run your own software distribution you might do things differently, or not have certain compilers available.

As an alternative to MSVC, conda-forge provides a MinGW-based compiler stack for Windows. This can be useful when porting Unix software that relies on GCC-specific features or when you want to avoid MSVC licensing requirements.

### Using MinGW compilers in recipes

To use the MinGW compiler stack, use the following compiler macros in your recipe:

```yaml
requirements:
  build:
    - ${{ compiler('m2w64_c') }}      # C compiler (gcc)
    - ${{ compiler('m2w64_cxx') }}    # C++ compiler (g++)
    - ${{ compiler('m2w64_fortran') }} # Fortran compiler (gfortran)
    - ${{ stdlib('m2w64_c') }}        # MinGW C standard library
```

These compilers correspond to the `gcc`, `gxx`, and `gfortran` packages from the MSYS2/MinGW-w64 ecosystem.

### ABI compatibility warning

The MinGW C++ and Fortran compilers are **not ABI-compatible** with the default MSVC stack. This means:

- You cannot mix libraries compiled with MinGW and MSVC in the same application
- Executables built with MinGW may link to MinGW runtime libraries (`libgcc`, `libwinpthread`, `libgomp`)
- Special care is needed when performing cross-library calls between MinGW and MSVC code

### When to use MinGW vs MSVC

| Use MinGW when...                                  | Use MSVC when...                               |
| -------------------------------------------------- | ---------------------------------------------- |
| Porting Unix/Linux software with GCC-specific code  | Building native Windows applications           |
| The project uses GNU autotools extensively         | Integrating with other MSVC-compiled libraries |
| You need GCC-specific compiler extensions           | Maximum compatibility with Windows ecosystem   |
| Building Fortran code (simpler than Flang setup)   | Performance-critical Windows applications      |
| Building R packages (requires MingW64)             |                                                |

### Legacy packages

Note that the older `m2w64-*` compiler packages (with the exception of `m2w64-sysroot`) are obsolete and no longer updated. Use the compiler macros shown above for new recipes.

## Clang compiler on Windows

Clang can be used as an alternative to both MSVC and MinGW on Windows. The `clang` compiler package installs two frontends, and conda-forge provides separate activation scripts for each.

Note that while Clang replaces the compiler itself, you still need the **Windows SDK** and **MSVC runtime libraries** installed on your system. These are provided by Visual Studio or the VS Build Tools installer (see [Installing the correct MSVC compilers](#installing-the-correct-msvc-compilers)). Clang uses the Windows SDK headers and links against the MSVC runtime libraries to produce Windows-compatible binaries.

### clang vs clang-cl

| Frontend   | Argument syntax      | Use case                                 |
| ---------- | -------------------- | ---------------------------------------- |
| `clang`    | GCC-style arguments  | Cross-platform builds, porting from Unix |
| `clang-cl` | MSVC-style arguments | Drop-in replacement for MSVC's `cl.exe`  |

### Using Clang in recipes

To use Clang on Windows (with `clang-cl` frontend) and other platforms (with standard `clang` frontend), use:

```yaml
requirements:
  build:
    - ${{ compiler('clang') }}    # C compiler
    - ${{ compiler('clangxx') }}  # C++ compiler
    - ${{ stdlib('c') }}
```

### Selecting a specific frontend on Windows

To explicitly select a frontend on Windows, configure your `variant_config.yaml`:

```yaml
# Use clang with GCC argument syntax
c_compiler:
  - clang
cxx_compiler:
  - clangxx

# Or use clang-cl with MSVC argument syntax
c_compiler:
  - clang-cl
cxx_compiler:
  - clang-cl
```

The `clang-cl` frontend is particularly useful when:

- You want Clang's diagnostics and optimizations but need MSVC ABI compatibility
- The build system expects MSVC-style compiler flags
- You're integrating with existing MSVC-compiled libraries

