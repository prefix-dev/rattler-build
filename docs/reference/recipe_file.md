# The recipe spec

`rattler-build` implements a new recipe spec, different from the traditional
"`meta.yaml`" file used in `conda-build`. A recipe has to be stored as a
`recipe.yaml` file.

## History

A discussion was started on what a new recipe spec could or should look like.
The fragments of this discussion can be found [here](https://github.com/mamba-org/conda-specs/blob/master/proposed_specs/recipe.md).

The reason for a new spec are:

- make it easier to parse (i.e. "pure YAML"); `conda-build` uses a mix of comments
  and Jinja to achieve a great deal of flexibility, but it's hard to parse the
  recipe with a computer
- iron out some inconsistencies around multiple outputs (`build` vs. `build/script`
  and more)
- remove any need for recursive parsing & solving
- finally, the initial implementation in `boa` relied on `conda-build`;
  `rattler-build` removes any dependency on Python or `conda-build` and
  reimplements everything in Rust

## Major differences from `conda-build`

- recipe filename is `recipe.yaml`, not `meta.yaml`
- outputs have less complicated behavior, keys are same as top-level recipe
  (e.g. `build/script`, not just `script` and `package/name`, not just `name`)
- no implicit meta-packages in outputs
- no full Jinja2 support: no conditional or `{% set ...` support, only string
  interpolation; variables can be set in the toplevel "context" which is valid
  YAML
- Jinja string interpolation needs to be preceded by a dollar sign at the
  beginning of a string, e.g. `- ${{ version }}` in order for it to be valid
  YAML
- selectors use a YAML dictionary style (vs. comments in conda-build). Instead
  of `- somepkg  #[osx]` we use:
  ```yaml
  if: osx
  then:
    - somepkg
  ```

- `skip` instruction uses a list of skip conditions and not the selector syntax
  from `conda-build` (e.g. `skip: ["osx", "win and py37"]`)

## Spec

The recipe spec has the following parts:

- [x] `context`: to set up variables that can later be used in Jinja string
  interpolation
- [x] `package`: defines name, version etc. of the top-level package
- [x] `source`: points to the sources that need to be downloaded in order to
  build the recipe
- [x] `build`: defines how to build the recipe and what build number to use
- [x] `requirements`: defines requirements of the top-level package
- [x] `tests`: defines tests for the top-level package
- [x] `outputs`: a recipe can have multiple outputs. Each output can and should
  have a `package`, `requirements` and `test` section

## Spec reference

The spec is also made available through a JSON Schema (which is used for
validation).<br/>
The schema (and `pydantic` source file) can be found in this repository:
[`recipe-format`](https://github.com/prefix-dev/recipe-format)


See more in the [automatic linting](../automatic_linting.md) chapter.

<!--
Quick start (from conda-build)
------------------------------

You can use `boa convert meta.yaml` to convert an existing recipe from conda-build syntax to boa. The command will output the new recipe to stdout. To quickly save the result, you can use `boa convert meta.yaml > recipe.yaml` and run `boa build .`. Please note that the conversion process is working fine only for "simple" recipes and there will be some needed manual work to convert complex recipes.

-->

Examples
--------

```yaml title="recipe.yaml"
# this sets up "context variables" (in this case name and version) that
# can later be used in Jinja expressions
context:
  version: 1.1.0
  name: imagesize

# top level package information (name and version)
package:
  name: ${{ name }}
  version: ${{ version }}

# location to get the source from
source:
  url: https://pypi.io/packages/source/${{ name[0] }}/${{ name }}/${{ name }}-${{ version }}.tar.gz
  sha256: f3832918bc3c66617f92e35f5d70729187676313caa60c187eb0f28b8fe5e3b5

# build number (should be incremented if a new build is made, but version is not incrementing)
build:
  number: 1
  script: python -m pip install .

# the requirements at build and runtime
requirements:
  host:
    - python
    - pip
  run:
    - python

# tests to validate that the package works as expected
tests:
  - python:
      imports:
        - imagesize

# information about the package
about:
  homepage: https://github.com/shibukawa/imagesize_py
  license: MIT
  summary: 'Getting image size from png/jpeg/jpeg2000/gif file'
  description: |
    This module analyzes jpeg/jpeg2000/png/gif image header and
    return image size.
  repository: https://github.com/shibukawa/imagesize_py
  documentation: https://pypi.python.org/pypi/imagesize

# the below is conda-forge specific!
extra:
  recipe-maintainers:
    - somemaintainer

```


### Package section

Specifies package information.

```yaml
package:
  name: bsdiff4
  version: "2.1.4"
```

- **name**: The lower case name of the package. It may contain "`-`", but no
  spaces.
- **version**: The version number of the package. Use the PEP-386 verlib
  conventions. Cannot contain "`-`". YAML interprets version numbers such as 1.0
  as floats, meaning that 0.10 will be the same as 0.1. To avoid this, put the
  version number in quotes so that it is interpreted as a string.


### Source section

Specifies where the source code of the package is coming from. The source may
come from a tarball file, `git`, `hg`, or `svn`. It may be a local path and it may
contain patches.


#### Source from tarball or `zip` archive

```yaml
source:
  url: https://pypi.python.org/packages/source/b/bsdiff4/bsdiff4-1.1.4.tar.gz
  md5: 29f6089290505fc1a852e176bd276c43
  sha1: f0a2c9a30073449cfb7d171c57552f3109d93894
  sha256: 5a022ff4c1d1de87232b1c70bde50afbb98212fd246be4a867d8737173cf1f8f
```

If an extracted archive contains only 1 folder at its top level, its contents
will be moved 1 level up, so that the extracted package contents sit in the root
of the work folder.

##### Specifying a file name

For URL and local paths you can specify a file name. If the source is an archive and a file name is set, automatic extraction is disabled.

```yaml
source:
  url: https://pypi.python.org/packages/source/b/bsdiff4/bsdiff4-1.1.4.tar.gz
  # will put the file in the work directory as `bsdiff4-1.1.4.tar.gz`
  file_name: bsdiff4-1.1.4.tar.gz
```

#### Source from `git`

```yaml
source:
  git: https://github.com/ilanschnell/bsdiff4.git
  # branch: master # note: defaults to fetching the repo's default branch
```

You can use `rev` to pin the commit version directly:

```yaml
source:
  git: https://github.com/ilanschnell/bsdiff4.git
  rev: "50a1f7ed6c168eb0815d424cba2df62790f168f0"
```

Or you can use the `tag`:

```yaml
source:
  git: https://github.com/ilanschnell/bsdiff4.git
  tag: "1.1.4"
```

`git` can also be a relative path to the recipe directory:

```yaml
source:
  git: ../../bsdiff4/.git
  tag: "1.1.4"
```

Furthermore, if you want to fetch just the current "`HEAD`" (this may result in
non-deterministic builds), then you can use `depth`.

```yaml
source:
  git: https://github.com/ilanschnell/bsdiff4.git
  depth: 1 # note: the behaviour defaults to -1
```

Note: `tag` or `rev` may not be available within commit depth range, hence we don't
allow using `rev` or the `tag` and `depth` of them together if not set to `-1`.

```yaml
source:
  git: https://github.com/ilanschnell/bsdiff4.git
  tag: "1.1.4"
  depth: 1 # error: use of `depth` with `rev` is invalid, they are mutually exclusive
```

When you want to use `git-lfs`, you need to set `lfs: true`. This will also pull
the `lfs` files from the repository.

```yaml
source:
  git: ../../bsdiff4/.git
  tag: "1.1.4"
  lfs: true # note: defaults to false
```

#### Source from a local path

If the path is relative, it is taken relative to the recipe directory. The
source is copied to the work directory before building.

```yaml
  source:
    path: ../src
    use_gitignore: false # note: defaults to true
```

By default, all files in the local path that are ignored by `git` are also ignored
by `rattler-build`. You can disable this behavior by setting `use_gitignore` to
`false`.

#### Patches

Patches may optionally be applied to the source.

```yaml
  source:
    #[source information here]
    patches:
      - my.patch # the patch file is expected to be found in the recipe
```

<!-- boa (conda-build) automatically determines the patch strip level. -->

#### Destination path

Within `rattler-build`'s work directory, you may specify a particular folder to
place the source into. `rattler-build` will always drop you into the same folder
(`[build folder]/work`), but it's up to you whether you want your source extracted
into that folder, or nested deeper. This feature is particularly useful when dealing
with multiple sources, but can apply to recipes with single sources as well.

```yaml
source:
  #[source information here]
  target_directory: my-destination/folder
```

#### Source from multiple sources

Some software is most easily built by aggregating several pieces.

The syntax is a list of source dictionaries. Each member of this list follows
the same rules as the single source. All features for each member are supported.

Example:

```yaml
source:
  - url: https://package1.com/a.tar.bz2
    target_directory: stuff
  - url: https://package1.com/b.tar.bz2
    target_directory: stuff
  - git: https://github.com/mamba-org/boa
    target_directory: boa
```

Here, the two URL tarballs will go into one folder, and the `git` repo is checked
out into its own space. `git` will not clone into a non-empty folder.

### Include only certain files from source

While you can specify only the files you need from a source, `source.filter` gives you the option to filter with globs instead.

```yaml title="recipe.yaml"
source:
  path: /path/to/source
  filter:
    - list
    - of
    - globs
```

Glob patterns throughout the recipe file can also use a flexible `include` /
`exclude` pair, such as:

```yaml title="recipe.yaml"
source:
  path: /path/to/source
  filter:
    include:
      - include/**/*.h
    exclude:
      - include/**/private.h
```

## Build section

Specifies build information.

Each field that expects a path can also handle a glob pattern. The matching is
performed from the top of the build environment, so to match files inside your
project you can use a pattern similar to the following one:
`"**/myproject/**/*.txt"`. This pattern will match any `.txt` file found in your
project. Quotation marks (`""`) are required for patterns that start with a `*`.

Recursive globbing using `**` is also supported.

#### Build number and string

The build number should be incremented for new builds of the same version. The
number defaults to `0`. The build string cannot contain "`-`". The string defaults
to the default `rattler-build` build string plus the build number.

```yaml
build:
  number: 1
  string: abc
```

#### Dynamic linking

This section contains settings for the shared libraries and executables.

```yaml
build:
  dynamic_linking:
    rpath_allowlist: ["/usr/lib/**"]
```

### Script

By default, `rattler-build` uses a `build.sh` file on Unix (macOS and Linux) and a
`build.bat` file on Windows, if they exist in the same folder as the `recipe.yaml`
file. With the script parameter you can either supply a different filename or
write out short build scripts. You may need to use selectors to use different
scripts for different platforms.

```yaml
build:
  # A very simple build script
  script: pip install .

  # The build script can also be a list
  script:
    - pip install .
    - echo "hello world"
    - if: unix
      then:
        - echo "unix"
```

### Skipping builds

Lists conditions under which `rattler-build` should skip the build of this recipe.
Particularly useful for defining recipes that are platform-specific. By default,
a build is never skipped.

```yaml
build:
  skip:
    - win
    ...
```

### Architecture-independent packages

Allows you to specify "no architecture" when building a package, thus making it
compatible with all platforms and architectures. Architecture-independent packages
can be installed on any platform.

Assigning the `noarch` key as `generic` tells `conda` to not try any manipulation of
the contents.

```yaml
build:
  noarch: generic
```

`noarch: generic` is most useful for packages such as static JavaScript assets
and source archives. For pure Python packages (similar to `none-any` wheels)
that can run on any Python version, you can use the `noarch: python` value instead:

```yaml
build:
  noarch: python
```

!!! note
    At the time of this writing, `noarch` packages should not make use
    of preprocess-selectors: `noarch` packages are built with the directives which
    evaluate to `true` in the platform it is built on, which probably will result
    in incorrect/incomplete installation in other platforms.

### Include only certain files in the package

Sometimes you may want to include only a subset of the files installed by the
build process in your package. For this, the `files` key can be used. Only _new_
files are considered for inclusion (ie. files that were not in the host
environment beforehand).

```yaml title="recipe.yaml"
build:
  # select files to be included in the package
  # this can be used to remove files from the package, even if they are installed in the
  # environment
  files:
    - list
    - of
    - globs
```

Glob patterns throughout the recipe file can also use a flexible `include` /
`exclude` pair, such as:

```yaml title="recipe.yaml"
build:
  files:
    include:
      - include/**/*.h
    exclude:
      - include/**/private.h
```


### Python specific options

#### Entry points

The following example creates a Python entry point named "`bsdiff4`" that calls
``bsdiff4.cli.main_bsdiff4()``. This is needed in [`noarch: python` packages](#architecture-independent-packages) to create
OS specific entry points at installation time.

```yaml
build:
  python:
    entry_points:
      - bsdiff4 = bsdiff4.cli:main_bsdiff4
      - bspatch4 = bsdiff4.cli:main_bspatch4
```

#### Version independent (ABI3) packages

Since rattler-build 0.35.0 and [CEP 20](https://github.com/conda/ceps/blob/main/cep-0020.md)
you can create version-independent Python packages that still contain compiled code.

ABI3 packages support building a native Python extension using a specific Python
version and running it against any later Python version. ABI3 or stable ABI is
supported by only CPython - the reference Python implementation with the Global
Interpreter Lock (GIL) enabled.

```yaml
build:
  python:
    version_independent: true  # defaults to false
```

## Include build recipe

The recipe and rendered `recipe.yaml` file are included in
the `package_metadata` by default. You can disable this by passing
`--no-include-recipe` on the command line.

!!! note
    There are many more options in the build section. These additional options control
    how variants are computed, prefix replacements, and more.
    See the [full build options](../build_options.md) for more information.


## Requirements section

Specifies the build and runtime requirements. Dependencies of these requirements
are included automatically.

Versions for requirements must follow the `conda`/`mamba` match specification. See
`build-version-spec`.

### Build

Tools required to build the package.

These packages are run on the build system and include things such as version
control systems (`git`, `svn`) make tools (GNU make, Autotool, CMake) and compilers
(real cross, pseudo-cross, or native when not cross-compiling), and any source
pre-processors.

Packages which provide "`sysroot`" files, like the `CDT` packages (see below), also
belong in the `build` section.

```yaml
requirements:
  build:
    - git
    - cmake
```

### Host

Represents packages that need to be specific to the target platform when the
target platform is not necessarily the same as the native build platform. For
example, in order for a recipe to be "cross-capable", shared libraries
requirements must be listed in the `host` section, rather than the `build` section,
so that the shared libraries that get linked are ones for the target platform,
rather than the native build platform. You should also include the base
interpreter for packages that need one. In other words, a Python package would
list `python` here and an R package would list `mro-base` or `r-base`.

```yaml
requirements:
  build:
    - ${{ compiler('c') }}
    - if: linux
      then:
        - ${{ cdt('xorg-x11-proto-devel') }}
  host:
    - python
```

!!! note
    When both "`build`" and "`host`" sections are defined, the `build` section can
    be thought of as "build tools" - things that run on the native platform, but
    output results for the target platform (e.g. a cross-compiler that runs on
    `linux-64`, but targets `linux-armv7`).


The `PREFIX` environment variable points to the host prefix. With respect to
activation during builds, both the host and build environments are activated.
The build prefix is activated before the host prefix so that the host prefix has
priority over the build prefix. Executables that don't exist in the host prefix
should be found in the build prefix.

The `build` and `host` prefixes are always separate when both are defined, or when
`${{ compiler() }}` Jinja2 functions are used. The only time that `build` and `host`
are merged is when the `host` section is absent, and no `${{ compiler() }}` Jinja2
functions are used in `meta.yaml`.

### Run

Packages required to run the package.

These are the dependencies that are installed automatically whenever the package is installed. Package names should follow the [package match
specifications](https://conda.io/projects/conda/en/latest/user-guide/concepts/pkg-specs.html#package-match-specifications).

```yaml
requirements:
  run:
    - python
    - six >=1.8.0
```

To build a recipe against different versions of NumPy and ensure that each
version is part of the package dependencies, list `numpy` as a requirement in
`recipe.yaml` and use a `conda_build_config.yaml` file with multiple NumPy
versions.

### Run constraints

Packages that are optional at runtime but must obey the supplied additional
constraint if they are installed.

Package names should follow the [package match
specifications](https://conda.io/projects/conda/en/latest/user-guide/concepts/pkg-specs.html#package-match-specifications).

```yaml
requirements:
  run_constraints:
    - optional-subpackage ==${{ version }}
```

For example, let's say we have an environment that has package "a" installed at
version 1.0. If we install package "b" that has a `run_constraints` entry of
"`a >1.0`", then `mamba` would need to upgrade "a" in the environment in order to
install "b".

This is especially useful in the context of virtual packages, where the
`run_constraints` dependency is not a package that `mamba` manages, but rather a
[virtual
package](https://docs.conda.io/projects/conda/en/latest/user-guide/tasks/manage-virtual.html)
that represents a system property that `mamba` can't change. For example, a
package on Linux may impose a `run_constraints` dependency on `__glibc >=2.12`.
This is the version bound consistent with CentOS 6. Software built against glibc
2.12 will be compatible with CentOS 6. This `run_constraints` dependency helps
`mamba`, `conda` or `pixi` tell the user that a given package can't be installed if their system
glibc version is too old.

### Run exports

Packages may have runtime requirements such as shared libraries (e.g. `zlib`), which are required for linking at build time, and for resolving the link at run time.
With `run_exports` packages runtime requirements can be implicitly added.
`run_exports` are weak by default, these two requirements for the `zlib` package are therefore equivalent:

```yaml title="recipe.yaml for zlib"
  requirements:
    run_exports:
      - ${{ pin_subpackage('libzlib', exact=True) }}
```

```yaml title="recipe.yaml for zlib"
  requirements:
    run_exports:
      weak:
        - ${{ pin_subpackage('libzlib', exact=True) }}
```

The alternative to `weak` is `strong`.
For `gcc` this would look like this:

```yaml title="recipe.yaml for gcc"
  requirements:
    run_exports:
      strong:
        - ${{ pin_subpackage('libgcc', exact=True) }}
```

`weak` exports will only be implicitly added as runtime requirement, if the package is a host dependency.
`strong` exports will be added for both build and host dependencies.
In the following example you can see the implicitly added runtime dependencies.

```yaml title="recipe.yaml of some package using gcc and zlib"
  requirements:
    build:
      - gcc            # has a strong run export
    host:
      - zlib           # has a (weak) run export
      # - libgcc       <-- implicitly added by gcc
    run:
      # - libgcc       <-- implicitly added by gcc
      # - libzlib      <-- implicitly added by libzlib
```


### Ignore run exports

There maybe cases where an upstream package has a problematic `run_exports` constraint.
You can ignore it in your recipe by listing the upstream package name in the
`ignore_run_exports` section in `requirements`.

You can ignore them by package name, or by naming the runtime dependency directly.

```yaml
  requirements:
    ignore_run_exports:
      from_package:
        - zlib
```

Using a runtime dependency name:

```yaml
  requirements:
    ignore_run_exports:
      by_name:
        - libzlib
```

!!! note
    `ignore_run_exports` only applies to runtime dependencies coming from an upstream package.

## Tests section

`rattler-build` supports four different types of tests. The "script test" installs
the package and runs a list of commands. The "Python test" attempts to import a
list of Python modules and runs `pip check`. The "downstream test" runs the tests
of a downstream package that reverse depends on the package being built. And lastly,
the "package content test" checks if the built package contains the mentioned items.

The `tests` section is a list of these items:

```yaml
tests:
  - script:
      - echo "hello world"
    requirements:
      run:
        - pytest
    files:
      source:
        - test-data.txt

  - python:
      imports:
        - bsdiff4
      pip_check: true  # this is the default
  - downstream: numpy
```

### Script test

The script test has 3 top-level keys: `script`, `files` and `requirements`. Only
the `script` key is required.

#### Test commands

Commands that are run as part of the test.

```yaml
tests:
  - script:
      - echo "hello world"
      - bsdiff4 -h
      - bspatch4 -h
```

#### External scripts

You can also easily run a script from your recipe directory.
Note that your package should either depend on the interpreter (e.g. Python or R)
or you need to add a `requirements` section to the test that installs the interpreter.

```yaml
tests:
  - script: tests/run_test.py
  - script: tests/run_test.R
  - script: tests/run_test.sh
```

#### Extra test files

Test files that are copied from the source work directory into the temporary
test directory and are needed during testing (note that the source work
directory is otherwise not available at all during testing).

You can also include files that come from the `recipe` folder. They are copied
into the test directory as well.

At test execution time, the test directory is the current working directory.

```yaml
tests:
  - script:
      - ls
    files:
      source:
        - myfile.txt
        - tests/
        - some/directory/pattern*.sh
      recipe:
        - extra-file.txt
```

#### Test requirements

In addition to the runtime requirements, you can specify requirements needed
during testing. The runtime requirements that you specified in the "`run`" section
described above are automatically included during testing (because the built
package is installed as it regularly would be).

In the `build` section you can specify additional requirements that are only
needed on the build system for cross-compilation (e.g. emulators or compilers).

```yaml
tests:
  - script:
      - echo "hello world"
    requirements:
      build:
        - myemulator
      run:
        - nose
```

### Python tests

For this test type you can list a set of Python modules that need to be
importable. The test will fail if any of the modules cannot be imported.

The test will also automatically run `pip check` to check for any broken
dependencies. This can be disabled by setting `pip_check: false` in the YAML.


```yaml
tests:
  - python:
      imports:
        - bsdiff4
        - bspatch4
      pip_check: true  # can be left out because this is the default
      python_version: 3.12.*  # optional: use list for multiple versions, default resolves to environment
```

Internally this will write a small Python script that imports the modules:

```python
import bsdiff4
import bspatch4
```

### Perl tests

For this test type you can list a set of Perl modules that need to be
importable. The test will fail if any of the modules cannot be imported.

```yaml
tests:
  - perl:
      uses:
        - Call::Context
```

Internally this will write a small Perl script that imports the modules:

```perl
use Call::Context;
```

### R tests

For this test type you can list a set of R modules that need to be
importable. The test will fail if any of the modules cannot be imported.

```yaml
- r:
    libraries:
      - knitr
```

Internally this will write a small R script that imports the modules:

```r
library(knitr)
```

### Check for package contents

Checks if the built package contains the mentioned items. These checks are executed directly at
the end of the build process to make sure that all expected files are present in the package.

```yaml
tests:
  - package_contents:
      # checks for the existence of files inside $PREFIX or %PREFIX%
      # or, checks that there is at least one file matching the specified `glob`
      # pattern inside the prefix
      files:
        - etc/libmamba/test.txt
        - etc/libmamba
        - etc/libmamba/*.mamba.txt

      # For more advanced cases, you can use the expanded form with exists and not_exists:
      # files:
      #   exists:
      #     - etc/libmamba/test.txt
      #     - etc/libmamba
      #     - etc/libmamba/*.mamba.txt
      #   not_exists:
      #     - etc/libmamba/unwanted.txt

      # checks for the existence of `mamba/api/__init__.py` inside of the
      # Python site-packages directory (note: also see Python import checks)
      site_packages:
        - mamba.api


      # looks in $PREFIX/bin/mamba for unix and %PREFIX%\Library\bin\mamba.exe on Windows
      # note: also check the `commands` and execute something like `mamba --help` to make
      # sure things work fine
      bin:
        - mamba

      # enable strict mode: error if any file in the package is not matched by one of the globs
      # (default: false)
      strict: true

      # searches for `$PREFIX/lib/libmamba.so` or `$PREFIX/lib/libmamba.dylib` on Linux or macOS,
      # on Windows for %PREFIX%\Library\lib\mamba.dll & %PREFIX%\Library\bin\mamba.bin
      lib:
        - mamba

      # searches for `$PREFIX/include/libmamba/mamba.hpp` on unix, and
      # on Windows for `%PREFIX%\Library\include\libmamba\mamba.hpp`
      include:
        - libmamba/mamba.hpp
```

### Downstream tests

!!! warning
    Downstream tests are not yet implemented in `rattler-build`.

A downstream test can mention a single package that has a dependency on the package being built.
The test will install the package and run the tests of the downstream package with our current
package as a dependency.

Sometimes downstream packages do not resolve. In this case, the test is ignored.

```yaml
tests:
  - downstream: numpy
```


## Outputs section

Explicitly specifies packaging steps. This section supports multiple outputs, as
well as different package output types. The format is a list of mappings.

When using multiple outputs, certain top-level keys are "forbidden": `package`
and `requirements`. Instead of `package`, a top-level `recipe` key can be
defined. The `recipe.name` is ignored but the `recipe.version` key is used as
default version for each output. Other "top-level" keys are merged into each
output (e.g. the `about` section) to avoid repetition. Each output is a
complete recipe, and can have its own `build`, `requirements`, and `test`
sections.

```yaml
recipe:
  # the recipe name is ignored
  name: some
  version: 1.0

outputs:
  - package:
      # version is taken from recipe.version (1.0)
      name: some-subpackage

  - package:
      name: some-other-subpackage
      version: 2.0
```

Each output acts like an independent recipe and can have their own `script`,
`build_number`, and so on.

```yaml
outputs:
  - package:
      name: subpackage-name
    build:
      script: install-subpackage
```

If `script` lacks a file extension,
the appropriate extension for the platform will be appended,
e.g. the above will run `install-subpackage.sh` in `bash` on most platforms
and `install-subpackage.bat` in `cmd.exe` on Windows.

Each output is built independently. You should take care of not packaging the
same files twice.

### Subpackage requirements

Like a top-level recipe, a subpackage may have zero or more dependencies listed
as build, host or run requirements.

The dependencies listed as subpackage build requirements are available only
during the packaging phase of that subpackage.

```yaml
outputs:
  - package:
      name: subpackage-name
    requirements:
      build:
        - some-dep
      run:
        - some-dep
```

You can also use the `pin_subpackage` function to pin another output from the
same recipe.

```yaml
outputs:
  - package:
      name: libtest
  - package:
      name: test
    requirements:
      build:
        - ${{ pin_subpackage('libtest', upper_bound='x.x') }}
```

The outputs are topologically sorted by the dependency graph which is taking the
`pin_subpackage` invocations into account. When using `pin_subpackage(name,
exact=True)` a special behavior is used where the `name` package is injected as
a "variant" and the variant matrix is expanded appropriately. For example, when
you have the following situation, with a `variant_config.yaml` file that
contains `openssl: [1, 3]`:

```yaml
outputs:
  - package:
      name: libtest
    requirements:
      host:
        - openssl
  - package:
      name: test
    requirements:
      build:
        - ${{ pin_subpackage('libtest', exact=True) }}
```

Due to the variant config file, this will build two versions of `libtest`. We
will also build two versions of `test`, one that depends on `libtest (openssl
1)` and one that depends on `libtest (openssl 3)`.


## About section

Specifies identifying information about the package. The information displays in
the package server.

```yaml
about:
  homepage: https://example.com/bsdiff4
  license: BSD-3-Clause # (1)!
  license_file: LICENSE
  summary: binary diff and patch using the BSDIFF4-format
  description: |
    Long description of bsdiff4 ...
  repository: https://github.com/ilanschnell/bsdiff4
  documentation: https://docs.com
```

1.  Only the SPDX specifiers are allowed, more info here: [SPDX](https://spdx.org/licenses/)
    If you want another license type `LicenseRef-<YOUR-LICENSE>` can be used, e.g. `license: LicenseRef-Proprietary`

### License file

Adds a file containing the software license to the package metadata.
Many licenses require the license statement to be distributed with the package.
The filename is relative to the source or recipe directory. The value can be a
single filename or a YAML list for multiple license files. Values can also point
to directories with license information. Directory entries must end with a `/`
suffix (this is to lessen unintentional inclusion of non-license files; all the
directory's contents will be unconditionally and recursively added).

If a license file is found in both the source and recipe directories, the file from
the recipe directory is used (you should see a warning about this in the build log).

```yaml
about:
  license_file:
    - LICENSE
    - vendor-licenses/
```


## Extra section

A schema-free area for storing non-`conda`-specific metadata in standard YAML
form.

???+ Example "Example: To store recipe maintainers information"
    ```yaml
    extra:
      maintainers:
       - name of maintainer
    ```


## Templating with Jinja

`rattler-build` supports limited Jinja templating in the `recipe.yaml` file.

You can set up Jinja variables in the `context` section:

```yaml
context:
  name: "test"
  version: "5.1.2"
  # later keys can reference previous keys
  # and use jinja functions to compute new values
  major_version: ${{ version.split('.')[0] }}
  tests_to_skip:
    # fails for one reason
    - test_foo
    # fails for another reason
    - test_bar
```

Later in your `recipe.yaml` you can use these values in string interpolation
with Jinja:

```yaml
source:
  url: https://github.com/mamba-org/${{ name }}/v${{ version }}.tar.gz

tests:
  - script:
    - pytest -k "not (${{ tests_to_skip | join(" or ")" }})"
```

Jinja has built-in support for some common string manipulations.

In rattler-build, complex Jinja is completely disallowed as we try to produce
YAML that is valid at all times. So you should not use any `{% if ... %}` or
similar Jinja constructs that produce invalid YAML. Furthermore, instead of
plain double curly brackets Jinja statements need to be prefixed by `$`, e.g.
`${{ ... }}`:

```yaml
package:
  name: {{ name }}   # WRONG: invalid yaml
  name: ${{ name }} # correct
```

For more information, see the [Jinja template
documentation](https://jinja.palletsprojects.com/en/3.1.x/) and the list of
available environment variables [`env-vars`]().

Jinja templates are evaluated during the build process.
<!-- TODO: implement the command to do below
To retrieve a fully rendered `recipe.yaml`, use the `` command.
-->

#### Additional Jinja2 functionality in rattler-build

Besides the default Jinja2 functionality, additional Jinja functions are
available during the `rattler-build` process: `pin_compatible`, `pin_subpackage`,
and `compiler`.

The compiler function takes `c`, `cxx`, `fortran` and other values as argument
and automatically selects the right (cross-)compiler for the target platform.

```
build:
  - ${{ compiler('c') }}
```

The `pin_subpackage` function pins another package produced by the recipe with
the supplied parameters.

Similarly, the `pin_compatible` function will pin a package according to the
specified rules.

#### Pin expressions

`rattler-build` knows pin expressions. A pin expression can have a `lower_bound`,
`upper_bound` and `exact` value. A `upper_bound` and `lower_bound` are specified with a
string containing only `x` and `.`, e.g. `upper_bound="x.x.x"` would signify to pin
the given package to `<1.2.3` (if the package version is `1.2.2`, for example).

A pin with `lower_bound="x.x",upper_bound="x.x"` for a package of version `1.2.2` would
evaluate to `>=1.2,<1.3.0a0`.

If `exact=true`, then the `hash` is included, and the package is pinned exactly,
e.g. `==1.2.2 h1234`. This is a unique package variant that cannot exist more
than once, and thus is "exactly" pinned.

You can also hard-code version strings into `lower_bound` and `upper_bound`.
See the [Jinja Reference](./jinja.md) for more information.

#### Pin subpackage

Pin subpackage refers to another package from the same recipe file. It is
commonly used in the `build/run_exports` section to export a run export from the
package, or with multiple outputs to refer to a previous build.

It looks something like:

```yaml
package:
  name: mypkg
  version: "1.2.3"

requirements:
  run_exports:
    # this will evaluate to `mypkg <1.3`
    - ${{ pin_subpackage(name, upper_bound='x.x') }}
```

#### Pin compatible

Pin compatible lets you pin a package based on the version retrieved from the
variant file (if the pinning from the variant file needs customization).

For example, if the variant specifies a pin for `numpy: 1.11`, one can use
`pin_compatible` to relax it:

```yaml
requirements:
  host:
    # this will select numpy 1.11
    - numpy
  run:
    # this will export `numpy >=1.11,<2`, instead of the stricter `1.11` pin
    - ${{ pin_compatible('numpy', min_pin='x.x', upper_bound='x') }}
```

#### The env Jinja functions

You can access the current environment variables using the `env` object in
Jinja.

There are three functions:

- `env.get("ENV_VAR")` will insert the value of "ENV_VAR" into the recipe.
- `env.get("ENV_VAR", default="undefined")` will insert the value of `ENV_VAR`
  into the recipe or, if `ENV_VAR` is not defined, the specified default value
  (in this case "undefined")
- `env.exists("ENV_VAR")` returns a boolean true of false if the env var is set
  to any value

This can be used for some light templating, for example:

```yaml
build:
  string: ${{ env.get("GIT_BUILD_STRING") }}_${{ hash }}
```

#### `match` function

This function matches the first argument (the package version) against the second
argument (the version spec) and returns the resulting boolean. This only works for packages
defined in the "variant_config.yaml" file.

```yaml title="recipe.yaml"
match(python, '>=3.4')
```

For example, you could require a certain dependency only for builds against python 3.4 and above:

```yaml title="recipe.yaml"
requirements:
  build:
    - if: match(python, '>=3.4')
      then:
        - some-dep
```

With a corresponding variant config that looks like the following:

```yaml title="variant_config.yaml"
python: ["3.2", "3.4", "3.6"]
```

Example: [`match` usage example](https://github.com/prefix-dev/rattler-build/tree/main/examples/match_and_cdt/recipe.yaml)

#### `cdt` function

This function helps add Core Dependency Tree packages as dependencies by converting packages as required according to hard-coded logic.

```yaml
# on x86_64 system
cdt('package-name') # outputs: package-name-cos6-x86_64
# on aarch64 system
cdt('package-name') # outputs: package-name-cos6-aarch64
```

Example: [`cdt` usage example](https://github.com/prefix-dev/rattler-build/tree/main/examples/match_and_cdt/recipe.yaml)

## Preprocessing selectors

You can add selectors to any item, and the selector is evaluated in a
preprocessing stage. If a selector evaluates to `true`, the item is flattened
into the parent element. If a selector evaluates to `false`, the item is
removed.

Selectors can use `if ... then ... else` as follows:

```yaml
source:
  - if: not win
    then:
      - url: http://path/to/unix/source
    else:
      - url: http://path/to/windows/source

# or the equivalent with two if conditions:

source:
  - if: unix
    then:
      - url: http://path/to/unix/source
  - if: win
    then:
      - url: http://path/to/windows/source
```

A selector is a valid Python statement that is executed. You can read more about
them in the ["Selectors in recipes" chapter](../selectors.md).

The use of the Python version selectors, `py27`, `py34`, etc. is discouraged in
favor of the more general comparison operators. Additional selectors in this
series will not be added to `conda-build`.

Because the selector is any valid Python expression, complicated logic is
possible:

```yaml
- if: unix and not win
  then: ...
- if: (win or linux) and not py27
  then: ...
```

Lists are automatically "merged" upwards, so it is possible to group multiple
items under a single selector:

```yaml
tests:
  - script:
    - if: unix
      then:
      - test -d ${PREFIX}/include/xtensor
      - test -f ${PREFIX}/lib/cmake/xtensor/xtensorConfigVersion.cmake
    - if: win
      then:
      - if not exist %LIBRARY_PREFIX%\include\xtensor\xarray.hpp (exit 1)
      - if not exist %LIBRARY_PREFIX%\lib\cmake\xtensor\xtensorConfigVersion.cmake (exit 1)

# On unix this is rendered to:
tests:
  - script:
    - test -d ${PREFIX}/include/xtensor
    - test -f ${PREFIX}/lib/cmake/xtensor/xtensorConfigVersion.cmake
```


## Experimental features

!!! warning
    These are experimental features of `rattler-build` and may change or go away completely.

### Jinja functions

- [`load_from_file`](../experimental_features.md#load-from-files)
- [`git.*` functions](../experimental_features.md#git-functions)
