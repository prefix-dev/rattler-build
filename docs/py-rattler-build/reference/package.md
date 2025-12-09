# Package

Load, inspect, and test conda packages.

You can import the package classes from `rattler_build`:

```python
from rattler_build import Package, PackageTest, TestResult
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

## `PackageTest`

::: rattler_build.PackageTest
    options:
        members:
            - kind
            - index
            - as_python_test
            - as_commands_test
            - as_perl_test
            - as_r_test
            - as_ruby_test
            - as_downstream_test
            - as_package_contents_test
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
            - script
            - requirements_run
            - requirements_build
            - to_dict

### `PerlTest`

::: rattler_build.PerlTest
    options:
        members:
            - uses
            - to_dict

### `RTest`

::: rattler_build.RTest
    options:
        members:
            - libraries
            - to_dict

### `RubyTest`

::: rattler_build.RubyTest
    options:
        members:
            - requires
            - to_dict

### `DownstreamTest`

::: rattler_build.DownstreamTest
    options:
        members:
            - downstream
            - to_dict

### `PackageContentsTest`

::: rattler_build.PackageContentsTest
    options:
        members:
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
