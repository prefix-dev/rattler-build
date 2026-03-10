<h1>
  <a href="https://prefix.dev/tools/rattler-build">
    <img alt="banner" src="https://github.com/user-attachments/assets/456f8ef1-1c7b-463d-ad88-de3496b05db2">
  </a>
</h1>

# rattler_build_package

A library for creating conda packages from files and metadata, supporting .tar.bz2 and .conda formats.

This crate provides a flexible API for building conda packages either from recipe structures or from manually-constructed metadata. It handles:

- File collection and filtering
- Metadata generation (about.json, index.json, paths.json, etc.)
- Prefix placeholder detection
- File transformations (noarch python, symlinks, etc.)
- Archive creation (.tar.bz2 and .conda formats)
