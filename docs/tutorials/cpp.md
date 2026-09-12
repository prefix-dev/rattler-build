# Packaging a C++ package

This tutorial will guide you though making a C++ package with Rattler-Build.

## Building a Header-only Library

To build a package for the header-only library `xtensor`, you need to manage dependencies and ensure proper installation paths.

### Key Steps

1. **Dependencies**:
   Ensure `cmake`, `ninja`, and a `compiler` are available as dependencies.

2. **CMake Installation Prefix**:
   Use the `CMAKE_INSTALL_PREFIX` setting to instruct `CMake` to install the headers in the correct location.

   * **Unix Systems**:
       Follow the standard Unix prefix:
       ```sh
       $PREFIX/include
       $PREFIX/lib
       ```

   * **Windows Systems**:
     Use a Unix-like prefix but nested in a `Library` directory:
     ```sh
     $PREFIX/Library/include
     $PREFIX/Library/lib
     ```
     Utilize the handy variables `%LIBRARY_PREFIX%` and `%LIBRARY_BIN%` to guide `CMake` to install the headers and libraries correctly.

This approach ensures that the headers and libraries are installed in the correct directories on both Unix and Windows systems.

### Recipe
```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/xtensor-tutorial.yaml"
```

1. The `if:` condition allows the user to switch behavior of the build based on some checks like, the operating system.
2. The `compiler` function is used to get the C++ compiler for the build system.
3. The `run_constraints` section specifies the version range of a package which the package can run "with".
But which the package doesn't depend on itself.
4. The `include` section specifies the header file to tested for existence.
5. The `files` section specifies the files to be tested for existence, using a glob pattern.

!!! note "`CMAKE_ARGS`"
    It can be tedious to remember all the different variables one needs to pass to CMake to create the perfect build.
    The `cmake` package on conda-forge introduces the`CMAKE_ARGS` environment variable.
    This variable contains the necessary flags to make the package build correctly, also when cross-compiling from one machine to another.
    Therefore, it is often not necessary to pass any additional flags to the `cmake` command.
    However, because this is a tutorial we will show how to pass the necessary flags to `cmake` manually.

    For more information please refer to the [conda-forge documentation](https://conda-forge.org/docs/maintainer/knowledge_base/#how-to-enable-cross-compilation).

## Building a header-only library as `noarch: generic`

The `xtensor` recipe above builds one package per platform, even though a
header-only library contains no compiled code and the packaged files are
identical on every architecture. To avoid rebuilding the same headers over and
over, you can package a header-only library as a single, architecture-independent
[`noarch: generic`](../reference/recipe_file.md#architecture-independent-packages)
package instead.

There is one wrinkle: the install layout differs between operating systems.
On Unix systems, headers live in `$PREFIX/include`, while on Windows they are
expected in `%PREFIX%\Library\include` (i.e. `%LIBRARY_PREFIX%\include`).
A single `noarch` package cannot place its files in two different locations,
so the trick is to build *two* flavors of the `noarch: generic` package:

1. One built on Linux or macOS, with the headers in `include/`.
2. One built on Windows, with the headers in `Library/include/`.

Both flavors share the same package name and version. To make sure that the
solver installs the correct flavor on each operating system, each flavor
depends on a *virtual package*: the Unix flavor requires `__unix`, and the
Windows flavor requires `__win`. Virtual packages are automatically provided by
the installer based on the running system, so the Windows flavor is simply not
installable on Unix systems and vice versa.

The following recipe packages the headers of [`ml_dtypes`](https://github.com/jax-ml/ml_dtypes)
and is based on conda-forge's [`libml_dtypes-headers` feedstock](https://github.com/conda-forge/libml_dtypes-headers-feedstock):

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/libml-dtypes-headers.yaml"
```

1. `noarch: generic` makes this a single architecture-independent package that
   ends up in the `noarch` subdirectory of the channel and can be installed on
   any platform.
2. The `if: unix` selector is evaluated for the platform the package is built
   on, so the build script copies the headers to the correct location for each
   flavor.
3. The virtual package (`__unix` or `__win`) restricts on which operating
   systems this flavor of the package can be installed.
4. The `package_contents` test checks the correct (platform-dependent) location
   of the installed headers.

To create both flavors, run `rattler-build` once on a Unix machine and once on
a Windows machine (for example, in two CI jobs):

```bash
rattler-build build --recipe ./recipe.yaml
```

Even though both flavors have the same name and version, they do not clash:
the `__unix` / `__win` virtual package requirement becomes part of the build
variant, so each flavor automatically gets a distinct build string hash:

```txt
noarch/libml_dtypes-headers-0.5.4-h5600cae_0.conda  (depends on __unix)
noarch/libml_dtypes-headers-0.5.4-h061c3d5_0.conda  (depends on __win)
```

!!! note "On conda-forge"
    On conda-forge, this pattern is enabled by setting `noarch_platforms` in
    the `conda-forge.yml` of the feedstock:

    ```yaml title="conda-forge.yml"
    noarch_platforms:
      - linux_64
      - win_64
    ```

## Building A C++ application

In this example, we'll build `poppler`, a C++ application for manipulating PDF files from the command line.
The final package will install several tools into the `bin/` folder.
We'll use external build scripts and run actual scripts in the test.

### Key Steps

1. **Dependencies**:
    - **Build Dependencies**: These are necessary for the building process, including `cmake`, `ninja`, and `pkg-config`.
    - **Host Dependencies**: These are the libraries `poppler` links against, such as `cairo`, `fontconfig`, `freetype`, `glib`, and others.

2. **Compiler Setup**:
   We use the `compiler` function to obtain the appropriate C and C++ compilers.

3. **Build Script**:
   The `build.script` field points to an external script (`poppler-build.sh`) which contains the build commands.

4. **Testing**:
   Simple tests are included to verify that the installed tools (`pdfinfo`, `pdfunite`, `pdftocairo`) are working correctly by running them, and expecting an exit code `0`.

### Recipe

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/poppler.yaml"
```

1. The `compiler` jinja function to get the correct compiler for C and C++ on the build system.
2. These are all the dependencies that the library links against.
3. The script test just executes some of the installed tools to check if they
   are working. These can be as complex as you want. (`bash` or `cmd.exe`)

### External Build Script
We've defined an external build script in the recipe. This will be searched next
to the recipe by the file name given, or the default name `build.sh` on `unix` or
`build.bat` on windows are searched for.

```bash title="poppler-build.sh"
#! /bin/bash

extra_cmake_args=(
    -GNinja
    -DCMAKE_INSTALL_LIBDIR=lib
    -DENABLE_UNSTABLE_API_ABI_HEADERS=ON
    -DENABLE_GPGME=OFF
    -DENABLE_LIBCURL=OFF
    -DENABLE_LIBOPENJPEG=openjpeg2
    -DENABLE_QT6=OFF
    -DENABLE_QT5=OFF
    -DENABLE_NSS3=OFF
)

mkdir build && cd build

cmake ${CMAKE_ARGS} "${extra_cmake_args[@]}" \
    -DCMAKE_PREFIX_PATH=$PREFIX \
    -DCMAKE_INSTALL_PREFIX=$PREFIX \
    -DTIFF_INCLUDE_DIR=$PREFIX/include \
    $SRC_DIR

ninja

# The `install` command will take care of copying the files to the right place
ninja install
```


## Parsing the `rattler-build build` Output

When running the `rattler-build build` command, you might notice some interesting information in the output.
Our package will have some `run` dependencies, even if we didn't specify any.

These come from the `run-exports` of the packages listed in the `host` section of the recipe.
This is indicated by `"RE of [host: package]"` in the output.

For example, `libcurl` specifies that if you depend on it in the host section, you should also depend on it during runtime with specific version ranges.
This ensures proper linking to shared libraries.

```
Run dependencies:
╭───────────────────────┬──────────────────────────────────────────────╮
│ Name                  ┆ Spec                                         │
╞═══════════════════════╪══════════════════════════════════════════════╡
│ libcurl               ┆ >=8.5.0,<9.0a0 (RE of [host: libcurl])       │
│ fontconfig            ┆ >=2.14.2,<3.0a0 (RE of [host: fontconfig])   │
│ fonts-conda-ecosystem ┆ (RE of [host: fontconfig])                   │
│ lcms2                 ┆ >=2.16,<3.0a0 (RE of [host: lcms2])          │
│ gettext               ┆ >=0.21.1,<1.0a0 (RE of [host: gettext])      │
│ freetype              ┆ >=2.12.1,<3.0a0 (RE of [host: freetype])     │
│ openjpeg              ┆ >=2.5.0,<3.0a0 (RE of [host: openjpeg])      │
│ libiconv              ┆ >=1.17,<2.0a0 (RE of [host: libiconv])       │
│ cairo                 ┆ >=1.18.0,<2.0a0 (RE of [host: cairo])        │
│ libpng                ┆ >=1.6.42,<1.7.0a0 (RE of [host: libpng])     │
│ libzlib               ┆ >=1.2.13,<1.3.0a0 (RE of [host: zlib])       │
│ libtiff               ┆ >=4.6.0,<4.7.0a0 (RE of [host: libtiff])     │
│ libjpeg-turbo         ┆ >=3.0.0,<4.0a0 (RE of [host: libjpeg-turbo]) │
│ libglib               ┆ >=2.78.3,<3.0a0 (RE of [host: glib])         │
│ libcxx                ┆ >=16 (RE of [build: clangxx_osx-arm64])      │
╰───────────────────────┴──────────────────────────────────────────────╯
```

You can also see "linking" information in the output, for example on macOS:

```txt
[lib/libpoppler-glib.8.26.0.dylib] links against:
 ├─ @rpath/libgio-2.0.0.dylib
 ├─ @rpath/libgobject-2.0.0.dylib
 ├─ /usr/lib/libSystem.B.dylib
 ├─ @rpath/libglib-2.0.0.dylib
 ├─ @rpath/libpoppler.133.dylib
 ├─ @rpath/libfreetype.6.dylib
 ├─ @rpath/libc++.1.dylib
 ├─ @rpath/libpoppler-glib.8.dylib
 └─ @rpath/libcairo.2.dylib
```

Rattler-Build ensures that:

1. All shared libraries linked against are present in the run dependencies.
Missing libraries trigger an `overlinking` warning.
2. You don't require any packages in the host that you are not linking against.
This triggers an `overdepending` warning.
