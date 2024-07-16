# What is `rattler-build`?

`rattler-build` is a tool to build and package software so that it can be
installed on any operating system – with any compatible package manager such as
`mamba`, `conda`, or `rattler`. We are also intending for `rattler-build` to be
used as a library to drive builds of packages from any other recipe format in
the future.

### How does `rattler-build` work?

Building of packages consists of several steps. It all begins with a
`recipe.yaml` file that specifies how the package is to be built and what the
dependencies are. From the recipe file, `rattler-build` executes several steps:

1. **Rendering**:
Parse the recipe file and evaluate conditionals, Jinja expressions, and
variables, and variants.

2. **Fetch source**:
Retrieve specified source files, such as `.tar.gz` files, `git` repositories, local paths.
Additionally, this step will apply patches that can be specified alongside the source file.

3. **Install build environments**:
Download and install dependencies into temporary "host" and "build" workspaces.
Any dependencies that are needed at build time are installed in this step.

4. **Build source**:
Execute the build script to _build/compile_ the source code and _install_ it into the host environment.

5. **Prepare package files**:
Collect _all_ files that are new in the "host" environment and apply some transformations if necessary;
specifically, we edit the `rpath` on `Linux` and `macOS` to make binaries relocatable.

6. **Package**:
Bundle all the files in a package and write out any additional metadata into the `info/index.json`, `info/about.json`, and `info/paths.json` files.
This also creates the test files that are bundled with the package.

7. **Test**:
Run any tests specified in the recipe.
The package is considered _done_ if it passes all the tests, otherwise its moved to `broken/` in the output directory.

After this process, a package is created. This package can be uploaded to somewhere like a custom [prefix.dev](https://prefix.dev) private or public channel.

### How to run `rattler-build`

Running `rattler-build` is straightforward. It can be done on the command line:

```sh
rattler-build build --recipe myrecipe/recipe.yaml
```

A custom channel that is not conda-forge (the default) can be specified like so:

```sh
rattler-build build -c robostack --recipe myrecipe/recipe.yaml
```

You can also use the `--recipe-dir` argument if you want to build all the packages in a directory:

```sh
rattler-build build --recipe-dir myrecipes/
```

### Overview of a `recipe.yaml`

A `recipe.yaml` file is separated into multiple sections and can conditionally
include or exclude sections. Recipe files also support a limited amount of
string interpolation with Jinja (specifically `minijinja` in our case).

A simple example of a recipe file for the `zlib` package would look as follows:

```yaml title="recipe.yaml"
# variables from the context section can be used in the rest of the recipe
# in jinja expressions
context:
  version: 1.2.13

package:
  name: zlib
  version: ${{ version }}

source:
  url: http://zlib.net/zlib-${{ version }}.tar.gz
  sha256: b3a24de97a8fdbc835b9833169501030b8977031bcb54b3b3ac13740f846ab30

build:
  # build numbers can be set arbitrarily
  number: 0
  script:
    # build script to install the package into the $PREFIX (host prefix)
    - if: unix
      then:
      - ./configure --prefix=$PREFIX
      - make -j$CPU_COUNT
    - if: win
      then:
      - cmake -G "Ninja" -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH=%LIBRARY_PREFIX%
      - ninja install

requirements:
  build:
    # compiler is a special function.
    - ${{ compiler("c") }}
    # The following two dependencies are only needed on Windows,
    # and thus conditionally selected
    - if: win
      then:
        - cmake
        - ninja
    - if: unix
      then:
        - make
```

The sections of a recipe are:

| sections       | description                                                                                                                              |
|----------------|------------------------------------------------------------------------------------------------------------------------------------------|
| `context`      | Defines variables that can be used in the Jinja context later in the recipe (e.g. name and version are commonly interpolated in strings) |
| `package`      | This section defines the name and version of the package you are currently building and will be the name of the final output             |
| `source`       | Defines where the source code is going to be downloaded from and checksums                                                               |
| `build`        | Settings for the build and the build script                                                                                              |
| `requirements` | Allows the definition of build, host, run and run-constrained dependencies                                                               |
