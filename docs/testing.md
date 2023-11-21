# Testing packages

When you are developing a package, you should write tests for it.
The tests are automatically executed right after the package build has finished.

The tests from the test section are actually packaged _into_ your package and can
also be executed straight from the existing package. For this, we have the `test` subcommand:

```bash
rattler-build test --package-file ./xtensor-0.24.6-h60d57d3_0.tar.bz2
```

Running the above command will extract the package and create a clean environment
where the package and dependencies are installed. Then the tests are executed in 
this environment.

If you inspect the package contents, you would find the test files under
`info/test/*`.

## How tests are translated

The test section allows you to specify the following things:

```yaml
test:
  # commands to run to test the package. If any of the commands
  # returns with an error code, the test is considered failed.
  commands:
    - echo "Hello world"
    - pip check

  # This test section tries to import the Python modules and errors if it can't
  imports:
    - mypkg
    - mypkg.subpkg

  # additional requirements at test time (only in the target platform architecture)
  requires:
    - pip

  # Extra files to be copied to the test directory from the build dir (can be globs)
  files:
    - test.py
    - "*.sh"

  # Extra files to be copied to the test directory from the source directory (can be globs)
  source_files:
    - test_files/
```

The files from the `files` and `source_files` sections are copied into the
`info/test/` folder. The `commands` section is turned into a `run_test.sh`
or `run_test.bat` file, depending on the platform. For a `noarch` package,
both are created. The imports section is turned into a `run_test.py` script.

## Internals

When you are writing a test for your package, additional files are created and added to your package.

The files are:

- `run_test.sh`  (Unix)
- `run_test.bat` (Windows)
- `run_test.py`  (for the Python import tests)

These files are created under the `info/test` directory of the package.
Additionally, any `source_files` or `files` are also moved into this directory.

The tests are executed pointing to this directory as the current working directory.

The idea behind adding the tests into the package is that you can execute the tests independent
from building the package. That is also why we are shipping a `test` subcommand that takes
as input an existing package and executes the tests.
