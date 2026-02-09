# Package

Load, inspect, and test conda packages.

You can import the package classes from `rattler_build`:

```python
from rattler_build import Package, PythonTest, CommandsTest, PackageContentsTest, TestResult
```

The `tests` property returns a list of test objects that can be pattern matched (Python 3.10+):

```python
for test in pkg.tests:
    match test:
        case PythonTest() as py_test:
            print(f"Python imports: {py_test.imports}")
        case CommandsTest() as cmd_test:
            print(f"Commands: {cmd_test.script}")
        case PackageContentsTest() as pc_test:
            print(f"Package contents check, strict={pc_test.strict}")
```

## `Package`

::: rattler_build.Package
    options:
        members:
            - from_file
            - name
            - version
            - build_string
            - build_number
            - subdir
            - noarch
            - depends
            - constrains
            - license
            - license_family
            - timestamp
            - arch
            - platform
            - path
            - archive_type
            - filename
            - files
            - tests
            - test_count
            - run_test
            - run_tests
            - to_dict

## `TestResult`

::: rattler_build.TestResult
    options:
        members:
            - success
            - output
            - test_index

## Test Types

### `PythonTest`

::: rattler_build.PythonTest
    options:
        members:
            - index
            - imports
            - pip_check
            - python_version
            - to_dict

### `PythonVersion`

::: rattler_build.PythonVersion
    options:
        members:
            - as_single
            - as_multiple
            - is_none

### `CommandsTest`

::: rattler_build.CommandsTest
    options:
        members:
            - index
            - script
            - requirements_run
            - requirements_build
            - to_dict

### `PerlTest`

::: rattler_build.PerlTest
    options:
        members:
            - index
            - uses
            - to_dict

### `RTest`

::: rattler_build.RTest
    options:
        members:
            - index
            - libraries
            - to_dict

### `RubyTest`

::: rattler_build.RubyTest
    options:
        members:
            - index
            - requires
            - to_dict

### `DownstreamTest`

::: rattler_build.DownstreamTest
    options:
        members:
            - index
            - downstream
            - to_dict

### `PackageContentsTest`

::: rattler_build.PackageContentsTest
    options:
        members:
            - index
            - files
            - site_packages
            - bin
            - lib
            - include
            - strict
            - to_dict

### `FileChecks`

::: rattler_build.FileChecks
    options:
        members:
            - exists
            - not_exists

### `PathEntry`

::: rattler_build.PathEntry
    options:
        members:
            - relative_path
            - no_link
            - path_type
            - size_in_bytes
            - sha256
