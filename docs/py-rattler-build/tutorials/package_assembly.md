# Package Assembly

This tutorial teaches you how to create conda packages programmatically using the
`assemble_package` function. This is useful when you already have files staged and
want to package them directly, without going through the full recipe build process.

## When to Use `assemble_package`

Use `assemble_package` when:

- You have files already compiled/staged and just need to package them
- You're building packages in a custom CI/CD pipeline

For building from recipes, use `Stage0Recipe.from_yaml()` with `render()` and `run_build()` instead.

```python exec="1" source="above" session="package_assembly"
import tempfile
from pathlib import Path

from rattler_build import (
    ArchiveType,
    FileEntry,
    assemble_package,
    collect_files,
)
```

## Example 1: Minimal Package

Let's create the simplest possible package - just a name, version, and some files:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_assembly"
# Create temporary directories for our example
work_dir = Path(tempfile.mkdtemp())
files_dir = work_dir / "files"
output_dir = work_dir / "output"
files_dir.mkdir()
output_dir.mkdir()

# Create some example files to package
(files_dir / "bin").mkdir()
(files_dir / "bin" / "hello").write_text("#!/bin/bash\necho 'Hello, World!'")

# Create the package
output = assemble_package(
    name="hello",
    version="1.0.0",
    target_platform="linux-64",
    build_string="0",
    output_dir=output_dir,
    files_dir=files_dir,
)

print(f"Package created: {output.path.name}")
print(f"Identifier: {output.identifier}")
```

## Example 2: Package with Metadata

Add package metadata like license, homepage, and dependencies:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_assembly"
# Create new directories
files_dir2 = work_dir / "files2"
output_dir2 = work_dir / "output2"
files_dir2.mkdir()
output_dir2.mkdir()

# Create a Python package structure
(files_dir2 / "lib" / "python3.12" / "site-packages" / "mylib").mkdir(parents=True)
(files_dir2 / "lib" / "python3.12" / "site-packages" / "mylib" / "__init__.py").write_text(
    '"""My library."""\n__version__ = "2.0.0"\n'
)

# Create package with full metadata
output = assemble_package(
    name="mylib",
    version="2.0.0",
    target_platform="linux-64",
    build_string="py312_0",
    output_dir=output_dir2,
    files_dir=files_dir2,
    # Metadata
    homepage="https://github.com/example/mylib",
    license="MIT",
    license_family="MIT",
    summary="A demonstration library",
    description="This is a longer description of the library.",
    # Dependencies
    depends=["python >=3.12,<3.13", "numpy >=1.20"],
    constrains=["scipy >=1.0"],
    build_number=0,
)

print(f"Package: {output.path.name}")
print(f"Identifier: {output.identifier}")
```

## Example 3: Collecting Files with Glob Patterns

For more control over which files to include, use `collect_files`:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_assembly"
# Create a directory with mixed content
mixed_dir = work_dir / "mixed"
mixed_dir.mkdir()

# Create various files
(mixed_dir / "src").mkdir()
(mixed_dir / "src" / "main.py").write_text("print('main')")
(mixed_dir / "src" / "utils.py").write_text("print('utils')")
(mixed_dir / "src" / "__pycache__").mkdir()
(mixed_dir / "src" / "__pycache__" / "main.cpython-312.pyc").write_bytes(b"bytecode")
(mixed_dir / "tests").mkdir()
(mixed_dir / "tests" / "test_main.py").write_text("def test(): pass")
(mixed_dir / "README.md").write_text("# My Project")

# Use collect_files to select only Python source files, excluding pycache
files = collect_files(
    mixed_dir,
    include_globs=["**/*.py"],
    exclude_globs=["**/__pycache__/**", "**/tests/**"],
)

print("Files selected:")
for f in files:
    print(f"  {f.destination}")

# Now create a package with these files
output_dir3 = work_dir / "output3"
output_dir3.mkdir()

output = assemble_package(
    name="filtered-pkg",
    version="1.0.0",
    target_platform="noarch",
    build_string="py_0",
    output_dir=output_dir3,
    files=files,
    noarch="python",
)

print(f"\nPackage: {output.path.name}")
```

## Example 4: Reproducible Builds with Timestamps

For reproducible builds, set a fixed timestamp:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_assembly"
import hashlib

# Create simple files
repro_dir = work_dir / "repro"
repro_dir.mkdir()
(repro_dir / "data.txt").write_text("Hello")

output_dir4 = work_dir / "output4"
output_dir4.mkdir()

# Fixed timestamp: 2024-01-01 00:00:00 UTC in milliseconds
FIXED_TIMESTAMP = 1704067200000

# Build twice with the same timestamp
output1 = assemble_package(
    name="repro-test",
    version="1.0.0",
    target_platform="noarch",
    build_string="0",
    output_dir=output_dir4,
    files_dir=repro_dir,
    timestamp=FIXED_TIMESTAMP,
    noarch="generic",
)

# Rename first package to avoid overwrite
output1.path.rename(output_dir4 / "first.conda")

output2 = assemble_package(
    name="repro-test",
    version="1.0.0",
    target_platform="noarch",
    build_string="0",
    output_dir=output_dir4,
    files_dir=repro_dir,
    timestamp=FIXED_TIMESTAMP,
    noarch="generic",
)

# Compare hashes
hash1 = hashlib.sha256((output_dir4 / "first.conda").read_bytes()).hexdigest()[:16]
hash2 = hashlib.sha256(output2.path.read_bytes()).hexdigest()[:16]

print(f"First build hash:  {hash1}")
print(f"Second build hash: {hash2}")
print(f"Reproducible: {hash1 == hash2}")
```

## Archive Formats

You can choose between `.conda` (modern, recommended) and `.tar.bz2` (legacy) formats:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_assembly"
print("Available archive types:")
print(f"  ArchiveType.Conda  -> {ArchiveType.Conda.extension()}")
print(f"  ArchiveType.TarBz2 -> {ArchiveType.TarBz2.extension()}")

# Create a .tar.bz2 package
output_dir5 = work_dir / "output5"
output_dir5.mkdir()

output = assemble_package(
    name="legacy-format",
    version="1.0.0",
    target_platform="linux-64",
    build_string="0",
    output_dir=output_dir5,
    files_dir=repro_dir,
    archive_type=ArchiveType.TarBz2,
)

print(f"\nCreated: {output.path.name}")
```

## Summary

| Function/Class | Purpose |
|---------------|---------|
| `assemble_package()` | Main function to create packages from files |
| `collect_files()` | Collect files with glob patterns |
| `FileEntry` | Represent a single file with source/destination paths |
| `ArchiveType` | Choose `.conda` or `.tar.bz2` format |
| `PackageOutput` | Result with `path` and `identifier` |

### Key Parameters for `assemble_package()`

**Required:**
- `name`, `version`, `target_platform`, `build_string`, `output_dir`
- At least one of `files_dir` or `files`

**Metadata (optional):**
- `homepage`, `license`, `license_family`, `summary`, `description`

**Dependencies (optional):**
- `depends`, `constrains`, `build_number`, `noarch`

**Build options (optional):**
- `compression_level` (0-9), `archive_type`, `timestamp`, `detect_prefix`
