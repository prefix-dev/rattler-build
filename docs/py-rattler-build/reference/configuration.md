# Configuration

Configuration classes for controlling build behavior and platform settings.

You can import the configuration classes from `rattler_build`:

```python
from rattler_build import ToolConfiguration, PlatformConfig, JinjaConfig, RenderConfig
```

## `ToolConfiguration`

::: rattler_build.ToolConfiguration
    options:
        members:
            - __init__
            - keep_build
            - test_strategy
            - skip_existing
            - continue_on_failure
            - channel_priority
            - use_zstd
            - use_bz2
            - use_sharded
            - compression_threads
            - io_concurrency_limit
            - allow_insecure_host
            - error_prefix_in_binary
            - allow_symlinks_on_windows

## `PlatformConfig`

::: rattler_build.PlatformConfig
    options:
        members:
            - __init__
            - target_platform
            - build_platform
            - host_platform
            - experimental
            - recipe_path

## `JinjaConfig`

::: rattler_build.JinjaConfig
    options:
        members:
            - __init__
            - target_platform
            - host_platform
            - build_platform
            - experimental
            - allow_undefined
            - variant
            - recipe_path
