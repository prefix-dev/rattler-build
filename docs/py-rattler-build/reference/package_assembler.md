# Package Assembler

Create conda packages programmatically without a recipe file.

You can import the package assembler classes and functions from `rattler_build`:

```python
from rattler_build import assemble_package, collect_files, ArchiveType, FileEntry, PackageOutput
```

## `assemble_package`

::: rattler_build.assemble_package

## `collect_files`

::: rattler_build.collect_files

## `ArchiveType`

::: rattler_build.ArchiveType
    options:
        members:
            - TarBz2
            - Conda
            - extension

## `FileEntry`

::: rattler_build.FileEntry
    options:
        members:
            - from_paths
            - source
            - destination
            - is_symlink
            - symlink_target

## `PackageOutput`

::: rattler_build.PackageOutput
    options:
        members:
            - path
            - identifier
