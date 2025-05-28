# Testing packages

When you are developing a package, you should write tests for it. The tests are
automatically executed as soon as the package build and all it's run dependencies
are ready.

## Writing tests

You can add one or more tests to your package in the `tests` section of the recipe (or output).
Each test is run independently, in a separate environment.

One notable difference are the `package_contents` tests that are executed right after the package
is prepared and do not create a new environment (as we only analyze the contents of the package).

```yaml title="recipe.yaml"
tests:
  # commands to run to test the package. If any of the commands
  # returns with an error code, the test is considered failed.
  - script:
      - echo "Hello world"
      - exit 1  # this will fail the test

  # run a script from the recipe folder
  - script: sometest.py

  # run a Python script with the Python interpreter
  - script:
      interpreter: python
      content: |
        import mypkg
        assert mypkg.__version__ == "0.1.0"

  # execute `pytest` with the tests from the `tests` folder
  - script:
      - pytest ./tests
    # additional requirements at test time
    requirements:
      run:
        - pytest
        - python 3.9.*  # require an older python version
    # extra files to be copied to the test folder from the recipe or source directory
    files:
      recipe:
        - tests/

  # python specific tests
  - python:
      # this test section tries to import the python modules and errors if it can't
      imports:
        - mypkg
      pip_check: true
      python_version: [3.9.*, 3.10.*]  # run against multiple older python versions

  - r:
      libraries:
        - dplyr

  - perl:
      modules:
        - JSON

  # test the contents of the package.
  - package_contents:
      files:
        - share/package/*.txt
        - lib/python*/site-packages/mypackage/*.py

  # test with strict mode: fails if there are any files not matched by the globs
  - package_contents:
      strict: true
      files:
        - share/package/*.txt
        - bin/myapp
      lib:
        - mylib
```

### Testing package contents

The `package_contents` test is a special test that is executed right after the
package is prepared. It does not create a new environment, but instead checks the paths that will be part of the final package.
It can be very useful as a "sanity check" to ensure that the package contains the expected files.

It has multiple sub-keys that help when building cross-platform packages:

- **`files`**: Specifies glob patterns for files that should exist in the package. You can provide a simple list of globs that should match at least one file in the package. If any pattern doesn't match at least one file, the test fails.

  > **Note**: For more advanced use cases, you can also use the expanded form with `exists` and `not_exists` fields:
  > ```yaml
  > files:
  >   exists:
  >     - share/package/*.txt
  >     - lib/python*/site-packages/mypackage/*.py
  >   not_exists:
  >     - lib/python*/site-packages/mypackage/deprecated_module.py
  > ```
- **`lib`**: matches libraries in the package (`.so`, `.dll`, `.dylib` files). The test fails if any of the libraries are not found. It's enough to specify the library name without any extension (e.g. `foo` will match `libfoo.so`, `libfoo.dylib`, and `foo.dll`).
- **`include`**: matches files under the `include` directory in the package. You can specify the file name like `foo.h`.
- **`bin`**: matches files under the `bin` directory in the package. You can specify executable names like `foo` which will match `foo.exe` on Windows and `foo` on Linux and macOS.
- **`site_packages`**: matches files under the `site-packages` directory in the package. You can specify the import path like `foobar.api` which will match `foobar/api.py` and `foobar/api/__init__.py`.
- **`strict`**: when set to `true`, enables strict mode. In strict mode, the test will fail if there are any files in the package that don't match any of the specified globs. (default: `false`).

## Testing existing packages

The tests from the test section are actually added _into_ your package and
can also be executed straight from the existing package.

The idea behind adding the tests into the package is that you can execute the
tests independently from building the package. That is also why we are shipping
a `test` subcommand that takes as input an existing package and executes the
tests:

```bash
rattler-build test --package-file ./xtensor-0.24.6-h60d57d3_0.tar.bz2
```

Running the above command will extract the package and create a clean
environment where the package and dependencies are installed. Then the tests are
executed in this newly-created environment.

If you inspect the package contents, you would find the test files under
`info/test/*`.

## How tests are translated

The `tests` section allows you to define test configurations for your package.
Tests are serialized to `info/tests/tests.yaml` in the created package and read from there during test execution.

When adding extra files to your tests:

1. **During package creation**
     - Files are copied to `$PREFIX/etc/conda/test-files/{pkg_name}/{idx}`
     - `{idx}` is a sequential number assigned to each test
     - Files can come from both `source` (work directory) and `recipe` locations
2. **During test execution**
     - Files are copied from `$PREFIX/etc/conda/test-files/{pkg_name}/{idx}` to a temporary directory
     - Tests run within this temporary directory
     - Use relative paths to access these files in your test commands

This approach ensures test files are properly packaged and available during test execution.

## Legacy tests

Legacy tests (from `conda-build`) are still supported for execution. These tests
are stored as files under the `info/test/` folder.

The files are:

- `run_test.sh` (Unix)
- `run_test.bat` (Windows)
- `run_test.py` (for the Python import tests)
- `test_time_dependencies.json` (for additional dependencies at test time)

Additionally, the `info/test/` folder contains all the files specified in the test
section as `source_files` and `files`. The tests are executed pointing to this
directory as the current working directory.
