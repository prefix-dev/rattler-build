# Windows quirks

Building packages on Windows is different from Unix systems (Linux and macOS). There are a few historical quirks, as well as the fact that the MSVC compiler is _not redistributable_ and has to be installed manually on the host system.

## The filesystem layout on Windows

The Windows filesystem layout is a bit different than the standard Unix one. This means where Unix recipes use `$PREFIX`, Windows should usually use `%LIBRARY_PREFIX%` which points to `%PREFIX%\Library\` instead of _just_ `%PREFIX%`.

Note: this is handled automatically when using conda-forge packages for CMake, Meson by using `%CMAKE_ARGS%` or `%MESON_ARGS% which include the correct values for the installation prefix.

On the top level, there is an additional `Scripts` folder, as well as a `bin/` folder. On Windows (unlike Unix systems), a total of 5 paths are added upon activation:

```bat
%PREFIX%\
%PREFIX%\Library\mingw-w64\bin"
%PREFIX%\Library\usr\bin"
%PREFIX%\Library\bin"
%PREFIX%\Scripts"
%PREFIX%\bin"
```

Additionally, the site-packages folder is _also_ located at the root of the filesystem layout:

```
- Library\
  - lib\
  - bin\
  - share\
  ...
- site-packages\
- Scripts\
- bin\
```

The reasons for this are ... ?? historical Python reasons.

## Build scripts


### Cmd.exe

The _default interpreter_ for build scripts on Windows is `cmd.exe` which has a quite clunky syntax and execution model. 

It will, for example, skip over errors if you do not manually insert `if %errorlevel% ...` after each statement. If the build script is a list of commands, then rattler-build will automatically inject this after each list item. If you pass in a complete build script or file, you will have to do this manually to recognize issues in command execution early on.

### Using Powershell

You can select PowerShell as an interpreter, which comes pre-installed on Windows these days. To do so, just set

```
build:
  script:
    interpreter: powershell
    script: ...
```

Or save your build script as `build.ps1` (which will automatically use powershell based on the file ending).

### Using `Bash` on Windows

To use bash on Windows, you can install bash in your build requirements (e.g. on conda-forge it would be `m2-bash`) and call the bash script from a cmd.exe script:

```
bash.exe -c $RECIPE_DIR/build.sh
```

## Installing the correct MSVC compilers

In order to install the correct MSVC compilers, you should get the Community [Visual Studio Installer](https://visualstudio.microsoft.com/downloads/).

The `C` / `C++` compiler that is installed on the host system needs to match the requirements of the recipe. For example, if the recipe uses `vs2022`, then you will need `Visual Studio Compilers 2022` installed on the host system. The same goes for `vs2026`, `vs2017`, etc. The installer also allows you to have multiple versions installed simultaneously. The "activation scripts" of the package will automatically _select_ the correct version by setting the environment variables properly.

You can use the GUI to install the right version of Visual Studio Compilers, or you can use the following commands in Powershell:

```powershell
# Download the installer (`/17/` will select vs2022)
Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vs_BuildTools.exe" -OutFile "$env:TEMP\vs_BuildTools.exe"

# Install C/C++ build tools silently
& "$env:TEMP\vs_BuildTools.exe" --quiet --wait --norestart --nocache `
    --add Microsoft.VisualStudio.Workload.VCTools `
    --includeRecommended
```

## MingW64 support in conda-forge

TODO...

