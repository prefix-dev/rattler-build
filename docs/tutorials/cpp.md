# Packaging a C++ package

This tutorial will guide you though making a C++ package with `rattler-build`.

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
context:
  version: "0.24.6"

package:
  name: xtensor
  version: ${{ version }}

source:
  url: https://github.com/xtensor-stack/xtensor/archive/${{ version }}.tar.gz
  sha256: f87259b51aabafdd1183947747edfff4cff75d55375334f2e81cee6dc68ef655

build:
  number: 0
  script:
    - if: win # (1)!
      then: |
        cmake -GNinja ^
            -D BUILD_TESTS=OFF -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% ^
            %SRC_DIR%
        ninja install
      else: |
        cmake -GNinja \
              -DBUILD_TESTS=OFF -DCMAKE_INSTALL_PREFIX=$PREFIX \
              $SRC_DIR
        ninja install

requirements:
  build:
    - ${{ compiler('cxx') }} # (2)!
    - cmake
    - ninja
  host:
    - xtl >=0.7,<0.8
  run:
    - xtl >=0.7,<0.8
  run_constraints: # (3)!
    - xsimd >=8.0.3,<10

tests:
  - package_contents:
      include: # (4)!
        - xtensor/xarray.hpp
      files: # (5)!
        exists:
          - ${{ "Library/" if win }}share/cmake/xtensor/xtensorConfig.cmake
          - ${{ "Library/" if win }}share/cmake/xtensor/xtensorConfigVersion.cmake

about:
  homepage: https://github.com/xtensor-stack/xtensor
  license: BSD-3-Clause
  license_file: LICENSE
  summary: The C++ tensor algebra library
  description: Multi dimensional arrays with broadcasting and lazy computing
  documentation: https://xtensor.readthedocs.io
  repository: https://github.com/xtensor-stack/xtensor

extra:
  recipe-maintainers:
    - some-maintainer
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
context:
  version: "24.01.0"

package:
  name: poppler
  version: ${{ version }}

source:
  url: https://poppler.freedesktop.org/poppler-${{ version }}.tar.xz
  sha256: c7def693a7a492830f49d497a80cc6b9c85cb57b15e9be2d2d615153b79cae08

build:
  script: poppler-build.sh

requirements:
  build:
    - ${{ compiler('c') }} # (1)!
    - ${{ compiler('cxx') }}
    - pkg-config
    - cmake
    - ninja
  host:
    - cairo # (2)!
    - fontconfig
    - freetype
    - glib
    - libboost-headers
    - libjpeg-turbo
    - lcms2
    - libiconv
    - libpng
    - libtiff
    - openjpeg
    - zlib

tests:
  - script:
      - pdfinfo -listenc  # (3)!
      - pdfunite --help
      - pdftocairo --help
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

When running the `rattler-build` command, you might notice some interesting information in the output.
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

`rattler-build` ensures that:

1. All shared libraries linked against are present in the run dependencies.
Missing libraries trigger an `overlinking` warning.
2. You don't require any packages in the host that you are not linking against.
This triggers an `overdepending` warning.
