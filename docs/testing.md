# Testing packages

The recipe allows for a test section. The tests from the test section are
actually packaged _into_ your package and can be executed straight from the
existing package. For this, we have a command:

```bash
rattler-build test ./mypkg-0.1.0-h60d57d3_0.tar.bz2
```

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
`info/test/` folder. The `commands` section is turned into a `run_commands.sh`
or `run_commands.bat` file, depending on the platform. For a `noarch` package,
both are created. The imports section is turned into a `run_test.py` script.
