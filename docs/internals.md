# What does `rattler-build` do to build a package?

`rattler-build` creates conda packages which are relocatable packages.
These packages are built up with some rules and conventions in mind.

## What goes into a package?

Generally speaking, any new files that are copied into the `$PREFIX` directory
at build time are part of the new package. However, there is some filtering
going on to exclude unwanted files, and `noarch: python` packages have special
handling as well. The rules are as follows:

### Filtering

#### General File Filtering

Certain files are filtered out to prevent them from being included in the
package. These include:

- **.pyo files**: Optimized Python files are not included [because they are
  considered harmful](https://www.python.org/dev/peps/pep-0488/).
- **.la files**: Libtool archive files that are not needed at runtime.
- **.DS_Store files**: macOS-specific files that are irrelevant to the package.
- **.git files and directories**: Version control files, including `.gitignore`
  and the `.git` directory, which are not needed in the package.
- **share/info/dir** This file is ignored because it would be written from
  multiple packages.

#### Special Handling for `noarch: python` Packages

For packages marked as `noarch: python`, special transformations are applied to
ensure compatibility across different platforms:

- **Stripping Python Library Prefix**: The "lib/pythonX.X" prefix is removed,
  retaining only the "site-packages" part of the path.
- **Skipping `__pycache__` Directories and `.pyc` Files**: These are excluded
  and recreated during installation (they are specific to the Python version).
- **Replacing `bin` and `Scripts` Directories**:
    - On Unix systems, the `bin` directory is replaced with `python-scripts`.
    - On Windows systems, the `Scripts` directory is replaced with
      `python-scripts`.
- **Remove explicitly mentioned entrypoints**: For `noarch: python` packages,
  entry points registered in the package are also taken into account. Files in
  the `bin` or `Scripts` directories that match entry points are excluded to
  avoid duplications.

### Symlink Handling

Symlinks are carefully managed to ensure they are relative rather than absolute,
which aids in making the package relocatable:

- Absolute symlinks pointing within the `$PREFIX` are converted to relative
  symlinks.
- On Unix systems, this conversion is handled directly by creating new relative
  symlinks.
- On Windows, a warning is issued since symlink creation requires administrator
  privileges.

## Making Packages Relocatable with `rattler-build`

Often, the most challenging aspect of building a package using `rattler-build`
is making it relocatable. A relocatable package can be installed into any
prefix, allowing it to be used outside the environment in which it was built.
This is in contrast to a non-relocatable package, which can only be utilized
within its original build environment.

`rattler-build` automatically performs the following actions to make packages
relocatable:

1. **Binary object file conversion**: Binary object files are converted to use
   relative paths using `install_name_tool` on macOS and `patchelf` on Linux.
   This uses `$ORIGIN` for elf files on Linux and `@loader_path` for Mach-O
   files on macOS to make the `rpath` relative to the executable / shared
   library.
2. **Text file prefix registration**: Any text file without `NULL` bytes
   containing the placeholder prefix have the registered prefix replaced with
   the install prefix.
3. **Binary file prefix detection and registration**: Binary files containing
the build prefix can be automatically registered. The registered files will have
their build prefix replaced with the install prefix at install time. This works
by padding the install prefix with null terminators, such that the length of the
binary file remains the same. The build prefix must be long enough to
accommodate any reasonable installation prefix. On macOS and Linux,
`rattler-build` pads the build prefix to 255 characters by appending
`_placehold` to the end of the build directory name.
<!--4. **Prefix replacement for specific binary files**: There may be cases where a
   file is identified as binary but requires the build prefix to be replaced as
   if it were text—without padding with null terminators. Such files can be
   listed in `build/has_prefix_files` in `meta.yaml`.-->
