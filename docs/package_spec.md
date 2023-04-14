# Package specification

`rattler-build` produces "conda" packages. These packages work with the `mamba`
and `conda` package managers, and they work cross-platform on Windows, Linux and
macOS.

By default, a conda package is a `tar.bz2` archive which contains

* Metadata under the `info/` directory.
* A collection of files that are installed directly into an install prefix.

The format is identical across platforms and operating systems. During the
install process, all files are extracted into the install prefix, with the
exception of the ones in `info/`. Installing a conda package into an environment
is similar to executing the following commands:

```bash
cd <environment prefix>
tar xjf mypkg-1.0.0-h2134.tar.bz2
```

Only files, including symbolic links, are part of a conda package. Directories
are not included. Directories are created and removed as needed, but you cannot
create an empty directory from the tar archive directly.

There is also a newer archive type, suffixed with `.conda`. This archive type
consists of an outer "zip" archive that is not compressed, and two inner
archives that are compressed with `zstd`, which is very fast for decompression. 

The inner archives are split into `info` and `pkg` files, which makes it
possible to extract only the `info` part of the archive (only the metadata),
which is often smaller in size.

### Package filename

A conda package conforms to the following filename:

```
<name>-<version>-<hash>.tar.bz2 OR <name>-<version>-<hash>.conda
```

## Package metadata

The `info/` directory contains all metadata about a package. Files in this
location are not installed under the install prefix. Although you are free to
add any file to this directory, conda only inspects the content of the files
discussed below.

### `info/index.json`

This file contains basic information about the package, such as name, version,
build string, and dependencies. The content of this file is stored in
`repodata.json`, which is the repository index file, hence the name
`index.json`. The JSON object is a dictionary containing the keys shown below. 


* `name` - string - The lowercase name of the package. May contain lowercase
characters, underscore and dashes.
* `version` - string - The package version. May not contain "-". Acknowledges
  [PEP 440](https://www.python.org/dev/peps/pep-0440/).
* `build` - string - The build string. May not contain "-". Differentiates
  builds of packages with otherwise identical names and versions, such as:
  * A build with other dependencies, such as Python 3.4 instead of Python 2.7.
  * A bug fix in the build process.
  * Some different optional dependencies, such as MKL versus ATLAS linkage.
    Nothing in conda actually inspects the build string. Strings such as
    `np18py34_1` are designed only for human readability and conda never parses
    them.
* `build_number` - integer - A non-negative integer representing the build
  number of the package. Unlike the build string, the `build_number` is
  inspected by conda. Conda uses it to sort packages that have otherwise
  identical names and versions to determine the latest one. This is important
  because new builds that contain bug fixes for the way a package is built may
  be added to a repository.
* `depends` - list of strings - A list of dependency specifications, where each
  element is a string, as outlined in :ref:`build-version-spec`.
* `constrains` - list of matchspecs - A list of optional dependency constraints.
  The packages listed under `constrains` are not installed by default, but if
  they are installed they have to respect the constraints.
* `subdir` - string - Optional. The subdir (like `linux-64`) of this package.
* `arch` - string - Optional. The architecture the package is built for.
  EXAMPLE: `x86_64`. This key is generally not used.
* `platform` - string - Optional. The OS that the package is built for. EXAMPLE:
  `osx`. This key is generally not used. Packages for a specific architecture
  and platform are usually distinguished by the repository subdirectory that
  contains them---see :ref:`repo-si`.

### `info/paths.json`

The
[`paths.json`](https://docs.rs/rattler_conda_types/latest/rattler_conda_types/package/struct.PathsJson.html)
file lists all files that are installed into the environment.

It consists of a list of [path
entries](https://docs.rs/rattler_conda_types/latest/rattler_conda_types/package/struct.PathsEntry.html),
each with the following keys:

* `_path` - string - the relative path of the file
* `path_type` - optional, string - the type of linking, can be `hardlink`,
  `softlink`, or `directory`. Default is `hardlink`.
* `file_mode` - optional, string - the file mode can be `binary` or `text`. This
  is only relevant for prefix replacement
* `prefix_placeholder` - optional, string - the prefix placeholder string that
  is encoded in the text or binary file, and that is replaced at installation
  time. Note that this prefix placeholder uses `/` even on Windows.
* `no_link` - bool, optional - whether or not this file should be linked or not
  when installing the package, defaults false (linking the file from the cache
  into the environment)
* `sha256` - string - the SHA256 hash of the file. For symbolic links it
  contains the SHA256 hash of the file pointed to.
* `size_in_bytes` - number - the size in bytes of the file. For symbolic links,
  it contains the file size of the file pointed to.

> Due to the way the binary replacement works, the placeholder prefix must be
> longer than the install prefix.

### `info/license/<...>`

All licenses mentioned in the recipe are copied to this folder.

### `info/about.json`

Optional file. Contains the entries of the about section of the recipe of the
`recipe.yaml` file. The following keys are added to `info/about.json` if present
in the build recipe:

* `home`
* `dev_url`
* `doc_url`
* `license_url`
* `license`
* `summary`
* `description`
* `license_family`



<!--

### `info/recipe/...`

A directory containing the full contents of the build recipe.

meta.yaml.rendered
------------------

The fully rendered build recipe. See :doc:`../resources/commands/conda-render`.

This directory is present only when the the `include_recipe` flag
is `True` in the :ref:`meta-build`.

-->