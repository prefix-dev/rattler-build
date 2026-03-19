# Debug Session

Interactive debugging for conda recipe builds. Set up the full build
environment without running the build, then iteratively run, inspect, and
modify the build script.

```python
from rattler_build import DebugSession, DebugPaths, ScriptResult
```

## `DebugSession`

::: rattler_build.debug.DebugSession
    options:
        members:
            - create
            - paths
            - work_dir
            - host_prefix
            - build_prefix
            - build_script
            - output_dir
            - setup_log
            - run_script
            - run
            - add_packages
            - create_patch
            - read_build_script

## `DebugPaths`

::: rattler_build.debug.DebugPaths
    options:
        members:
            - work_dir
            - host_prefix
            - build_prefix
            - build_script
            - build_env_script
            - build_dir
            - output_dir
            - recipe_dir

## `ScriptResult`

::: rattler_build.debug.ScriptResult
    options:
        members:
            - exit_code
            - stdout
            - stderr
            - success
