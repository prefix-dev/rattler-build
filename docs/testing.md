# Testing packages

When you are developing a package, you should write tests for it. The tests are
automatically executed right after the package build has finished.

The tests from the test section are actually packaged _into_ your package and
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

The `tests` section allows you to specify the following things:

```yaml
tests:
  - script:
      # commands to run to test the package. If any of the commands
      # returns with an error code, the test is considered failed.
      - echo "Hello world"
      - pytest ./tests

    # additional requirements at test time
    requirements:
      run:
        - pytest

    files:
      # Extra files to be copied to the test directory from the "work directory"
      source:
        - tests/
        - test.py
        - *.sh
      recipe:
        - more_tests/*.py

  # This test section tries to import the Python modules and errors if it can't
  - python:
      imports:
        - mypkg
        - mypkg.subpkg
```

When you are writing a test for your package, additional files are created and
added to your package. These files are placed under the `info/tests/{index}/`
folder for each test.

For a script test:

- All the files are copied straight into the test folder (under
  `info/tests/{index}/`)
- The script is turned into a `run_test.sh` or `run_test.bat` file
- The extra requirements are stored as a JSON file called
  `test_time_dependencies.json`

For a Python import test:

- A JSON file is created that is called `python_test.json` and stores the
  imports to be tested and whether to execute `pip check` or not. This file is
  placed under `info/tests/{index}/`

For a downstream test:

- A JSON file is created that is called `downstream_test.json` and stores the
  downstream tests to be executed. This file is placed under
  `info/tests/{index}/`

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
