# Advanced Build Options

There are some specialized build options to control various features:

- prefix replacement
- variant configuration
- encoded file type

These are all found under the `build` key in the `recipe.yaml`.

## Always include and always copy files

There are some options that control the inclusion of files in the final package.

The `always_include_files` option can be used to include files even if they are
already in the environment as part of some other host dependency. This is normally
"clobbering" and should be used with caution (since packages should not have any overlapping files).

The `always_copy_files` option can be used to copy files instead of linking them.
This is useful for files that might be modified inside the environment (for example configuration files).
Normally, files are linked from a central cache into the environment to save space – that means
that files modified in one environment will be modified in all environments. This is not always
desirable, and in that case you can use the `always_copy_files` option.

```yaml title="recipe.yaml"
build:
  # include files even if they are already in the environment
  # as part of some other host dependency
  always_include_files: [glob]

  # do not soft- or hard-link these files, but always copy them was `no_link`
  always_copy_files: [glob]
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
      text: [globs]
      # force binary file type (list of globs)
      binary: [globs]

    # ignore all or specific files for prefix replacement`
    ignore: bool | [path] (defaults to false)

    # wether to detect binary files with prefix or not
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

More about variants can be found in the [variant documentation](../variants.md).

The following options are available in the `build` section to control the
variant configuration:

```yaml title="recipe.yaml"
build:
  # settings for the variant
  variant:
    # Keys to forcibly use for the variant computation
    # even if they are not in the dependencies
    use_keys: [string]

    # Keys to forcibly ignore for the variant computation
    # even if they are in the dependencies
    ignore_keys: [string]

    # used to prefer this variant less
    down_prioritize_variant: integer (defaults to 0, higher is less preferred)

```

## Dynamic linking configuration

After the package is build, rattler-build performs some "post-processing" on the
binaries and libraries.

This entails making the shared libraries relocatable and checking that all
linked libraries are present in the run requirements. The following settings
control this behavior.

With the `rpath` option you can forcibly set the `rpath` of the shared
libraries. The path is relative to the install prefix. Any rpath setting is
ignored on Windows.

The `rpath_allowlist` option can be used to allow the `rpath` to point to
locations outside of the environment. This is useful if you want to link against
libraries that are not part of the conda environment (e.g. proprietary
software).

If you want to stop `rattler-build` from relocating the binaries, you can set
`binary_relocation` to `false`. If you want to only relocate some binaries, you
can select the relevant ones with a glob pattern.

If you link against some libraries (possibly even outside of the prefix, in a
system location), then you can use the `missing_dso_allowlist` to allow linking
against these and suppress any warnings. This list is pre-populated with a list
of known system libraries on the different operating systems.

As part of the post-processing, `rattler-build` checks for overlinking and
overdepending. Overlinking is when a binary links against a library that is not
specified in the run requirements. This is usually a mistake because the library
would not be present in the environment when the package is installed.

Conversely, overdepending is when a library is part of the run requirements, but
is not actually used by any of the binaries/libraries in the package.

```yaml title="recipe.yaml"
build:
  # settings for shared libraries and executables
  dynamic_linking:
    # linux only, list of rpaths relative to the installation prefix
    rpaths: [path] (defaults to ['lib/'])

    # Allow runpath / rpath to point to these locations
    # outside of the environment
    rpath_allowlist: [glob]

    # wether to relocate binaries or not. If this is a list of paths, then
    # only the listed paths are relocated
    binary_relocation: bool (defaults to true) | [glob]

    # Allow linking against libraries that are not in the run requirements
    missing_dso_allowlist: [glob]

    # what to do when detecting overdepending
    overdepending_behavior: "ignore" or "error" # (defaults to "error")

    # what to do when detecting overlinking
    overlinking_behavior: "ignore" or "error" # (defaults to "error")
```
