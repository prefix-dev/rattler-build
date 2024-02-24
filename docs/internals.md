# Internals of `rattler-build`

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
   This uses `$ORIGIN` for elf files on Linux and `@loader_path` for Mach-O files
   on macOS to make the `rpath` relative to the executable / shared library.
2. **Text file prefix registration**: Any text file without `NULL` bytes
   containing the placeholder prefix have the registered prefix replaced with the
   install prefix.
3. **Binary file prefix detection and registration**: Binary files containing
   the build prefix can be automatically registered. The registered files will
   have their build prefix replaced with the install prefix at install time.
   This works by padding the install prefix with null terminators, such that the
   length of the binary file remains the same. The build prefix must be long
   enough to accommodate any reasonable installation prefix. On macOS and Linux,
   `rattler-build` pads the build prefix to 255 characters by appending
   `_placehold` to the end of the build directory name.
<!--4. **Prefix replacement for specific binary files**: There may be cases where a
   file is identified as binary but requires the build prefix to be replaced as
   if it were text—without padding with null terminators. Such files can be
   listed in `build/has_prefix_files` in `meta.yaml`.-->
