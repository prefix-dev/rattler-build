# Advanced build options

There are some specialized build options to control various features:

- prefix replacement
- variant configuration
- encoded file type

These are all found under the `build` key in the `recipe.yaml`.

## Include only certain files in the package

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

For example, to only include the header files in a package, you could use:

```yaml title="recipe.yaml"
build:
  files:
    - include/**/*.h
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

### Glob evaluation

Glob patterns are used throughout the build options to specify files. The
patterns are matched against the relative path of the file in the build
directory. Patterns can contain `*` to match any number of characters, `?` to
match a single character, and `**` to match any number of directories.

For example:

- `*.txt` matches all files ending in `.txt`
- `**/*.txt` matches all files ending in `.txt` in any directory
- `**/test_*.txt` matches all files starting with `test_` and ending in `.txt`
  in any directory
- `foo/` matches all files under the `foo` directory

The globs are always evaluated relative to the prefix directory. If you have no
`include` globs, but an `exclude` glob, then all files are included except those
that match the `exclude` glob. This is equivalent to `include: ['**']`.

## Always include and always copy files

There are some options that control the inclusion of files in the final package.

The `always_include_files` option can be used to include files even if they are
already in the environment as part of some other host dependency. This is
normally "clobbering" and should be used with caution (since packages should not
have any overlapping files).

The `always_copy_files` option can be used to copy files instead of linking
them. This is useful for files that might be modified inside the environment
(e.g. configuration files). Normally, files are linked from a central cache into
the environment to save space – that means that files modified in one
environment will be modified in all environments. This is not always desirable,
and in that case you can use the `always_copy_files` option.

??? note "How `always_copy_files` works" The `always_copy_files` option works by
setting the `no_link` option in the `info/paths.json` to `true` for the files in
question. This means that the files are copied instead of linked when the
package is installed.

```yaml title="recipe.yaml"
build:
  # include files even if they are already in the environment
  # as part of some other host dependency
  always_include_files: list of globs

  # do not soft- or hard-link these files, but always copy them was `no_link`
  always_copy_files: list of globs
```

## Merge build and host environments

In very rare cases you might want to merge the build and host environments to
obtain the "legacy" behavior of conda-build.

```yaml title="recipe.yaml"
build:
  # merge the build and host environments (used in many R packages on Windows)
  merge_build_and_host_envs: bool (defaults to false)
```

## Prefix detection / replacement options

During installation time the "install"-prefix is injected into text and binary
files. Sometimes this is not desired, and sometimes the user might want closer
control over the automatic text/binary detection.

The main difference between prefix replacement for text and binary files is that
for binary files, the prefix string is padded with null bytes to match the
length of the original prefix. The original prefix is the very long placeholder
string that you might have seen in the build process.

On Windows, binary prefix replacement is never performed.

```yaml title="recipe.yaml"
package:
  name: mypackage
  version: 1.0

build:
  # settings concerning the prefix detection in files
  prefix_detection:
    # force the file type of the given files to be TEXT or BINARY
    # for prefix replacement
    force_file_type:
      # force TEXT file type (list of globs)
      text: list of globs
      # force binary file type (list of globs)
      binary: list of globs

    # ignore all or specific files for prefix replacement`
    ignore: bool | [path] (defaults to false)

    # whether to detect binary files with prefix or not
    # defaults to true on Unix and (always) false on Windows
    ignore_binary_files: bool
```

## Variant configuration

To control the variant precisely you can use the "variant configuration"
options.

A variant package has the same version number, but different "hash" and
potentially different dependencies or build options. Variant keys are extracted
from the `variant_config.yaml` file and usually any used Jinja variables or
dependencies without version specifier are used as variant keys.

Variant keys can also be forcibly set or ignored with the `use_keys` and
`ignore_keys` options.

In order to decide which of the variant packages to prefer and install by
default, the `down_prioritize_variant` option can be used. The higher the value,
the less preferred the variant is.

More about variants can be found in the [variant documentation](variants.md).

The following options are available in the `build` section to control the
variant configuration:

```yaml title="recipe.yaml"
build:
  # settings for the variant
  variant:
    # Keys to forcibly use for the variant computation
    # even if they are not in the dependencies
    use_keys: list of strings

    # Keys to forcibly ignore for the variant computation
    # even if they are in the dependencies
    ignore_keys: list of strings

    # used to prefer this variant less
    down_prioritize_variant: integer (defaults to 0, higher is less preferred)
```

## Dynamic linking configuration

After the package is built, rattler-build performs some "post-processing" on the
binaries and libraries.

This entails making the shared libraries relocatable and checking that all
linked libraries are present in the run requirements. The following settings
control this behavior.

With the `rpath` option you can forcibly set the `rpath` of the shared
libraries. The path is relative to the install prefix. Any `rpath` setting is
ignored on Windows.

The `rpath_allowlist` option can be used to allow the `rpath` to point to
locations outside of the environment. This is useful if you want to link against
libraries that are not part of the conda environment (e.g. proprietary
software).

If you want to stop `rattler-build` from relocating the binaries, you can set
`binary_relocation` to `false`. If you want to only relocate some binaries, you
can select the relevant ones with a glob pattern.

To read more about `rpath`s and how rattler-build creates relocatable binary
packages, see the [internals](internals.md) docs.

If you link against some libraries (possibly even outside of the prefix, in a
system location), then you can use the `missing_dso_allowlist` to allow linking
against these and suppress any warnings. This list is pre-populated with a list
of known system libraries on the different operating systems.

As part of the post-processing, `rattler-build` checks for overlinking and
overdepending. "Overlinking" is when a binary links against a library that is
not specified in the run requirements. This is usually a mistake because the
library would not be present in the environment when the package is installed.

Conversely, "overdepending" is when a library is part of the run requirements,
but is not actually used by any of the binaries/libraries in the package.

In addition to handling binary dependencies, `rattler-build` also ensures that
packages containing hardcoded paths into the environment are relocatable when
installed outside the of the build environment. To do this, `rattler-build`
constructs a host environment with a 255 character name of the form
`host_env_placehold[_placehold[_placehold[...]]]` (for details, see the
[internals](internals.md) docs). At install time, conda will find these paths
and replace them in binaries with the path to the environment being installed
into.

Since this process may not be safe for all packages, and not all packages will
require these modifications (if packages are already internally avoiding
embedding invalid absolute paths, for example), then this process may be
disabled using the `prefix_detection` options shown below.

```yaml title="recipe.yaml"
build:
  # settings for shared libraries and executables
  dynamic_linking:
    # linux only, list of rpaths relative to the installation prefix
    rpaths: list of paths (defaults to ['lib/'])

    # Allow runpath / rpath to point to these locations
    # outside of the environment
    rpath_allowlist: list of globs

    # whether to relocate binaries or not. If this is a list of paths, then
    # only the listed paths are relocated
    binary_relocation: bool (defaults to true) | list of globs

    # Allow linking against libraries that are not in the run requirements
    missing_dso_allowlist: list of globs

    # what to do when detecting overdepending
    overdepending_behavior: "ignore" or "error" # (defaults to "ignore")

    # what to do when detecting overlinking
    overlinking_behavior: "ignore" or "error" # (defaults to "ignore")

  prefix_detection:
    # A set of files to ignore prefix detection for altogether, see
    ignore: list of globs

    force_file_type:
      # Force replacement of files as binary blobs regardless of their type
      binary: list of globs
      # Force replacement of files as text (strings) regardless of their type
      text: list of globs
```

## Python options

There are some additional options in the `python` section of the `build` key.

The `entry_points` option can be used to specify entry points for the package.

The `use_python_app_entrypoint` option can be used to specify if `python.app`
which is useful for GUI applications on macOS.

The `skip_pyc_compilation` option can be used to exclude certain files from
being automatically compiled from `.py` to `.pyc`. Note that `noarch: python`
packages never contain `.pyc` files. Some packages ship .py files that cannot be
compiled, such as those that contain templates. Some packages also ship .py
files that should not be compiled yet, because the Python interpreter that will
be used is not known at build time. In these cases, conda-build can skip
attempting to compile these files. The patterns used in this section do not need
the ** to handle recursive paths.

The `site_packages_path` is a specific option that is only used when build
`python` itself. It will add metadata to the package record of the python
package to tell the installer where the `site-packages` path is located. This is
used to install noarch packages in the correct location.

```yaml title="recipe.yaml"
build:
  python:
    # entry points for the package
    entry_points:
      - bsdiff4 = bsdiff4.cli:main_bsdiff4
      - bspatch4 = bsdiff4.cli:main_bspatch4

    # use python.app entrypoint (macOS only)
    use_python_app_entrypoint: false  # (defaults to false, only used on macOS)

    # skip pyc compilation for certain files
    skip_pyc_compilation:
      - foo/*.py

    # Option to specify whether a package is version independent (aka ABI3)
    version_independent: true  # defaults to false
```

And an example of the `site_packages_path` option when building the python
interpreter:

```yaml title="recipe.yaml"
package:
  name: python
  version: "3.13.0"

build:
  python:
    # path to the site-packages folder
    site_packages_path: "lib/python3.13/site-packages"
```

### Python Package Version Independence

Conda packages can be made version-independent in two different ways:

#### `noarch: python`

Packages marked as `noarch: python` contain only pure Python code without
compiled extensions. These packages work across all Python versions and
platforms from a single build.

#### `version_independent: true`

Packages marked as version_independent support multiple Python versions while
containing compiled extensions using Python's ABI3 compatibility. These require
platform-specific builds (Windows, macOS, Linux) but remain compatible across
different Python versions within each platform.

## Post processing of the package contents (experimental)

rattler-build allows you to post-process the package contents with `regex`
replacements after the build has finished. This is only useful in very specific
cases when you cannot easily identify new files and want to run post-processing
only on new files.

Note that this is an experimental feature and might be removed or changed in the
future.

The `post_process` key is a list of dictionaries with the following keys:

- files: list of globs to select the files from the package that you want to
  modify
- regex: the regular expression to match in the file. Note that this uses Rust
  regex syntax.
- replacement: the replacement string to use. Attention: note that Rust supports
  expanding "named captures" with $name or ${name}. If you want to replace with
  a env variable, you need to use `$${name}` to get `${name}` in the output.

```yaml title="recipe.yaml"
build:
  post_process:
    - files:
        - *.txt
      regex: "foo"
      replacement: "bar"
    - files:
        - '*.pc'
      regex: (?:-L|-I)?"?([^;\s]+/sysroot/)
      replacement: '$${CONDA_BUILD_SYSROOT_S}'  # note this expands to `${CONDA_BUILD_SYSROOT_S}`
```
