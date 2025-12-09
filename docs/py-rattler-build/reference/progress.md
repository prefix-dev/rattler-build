# Progress

Progress callbacks for monitoring build operations.

You can implement the `ProgressCallback` protocol to receive progress updates,
or use one of the built-in implementations.

```python
from rattler_build.progress import (
    ProgressCallback,
    SimpleProgressCallback,
    RichProgressCallback,
    create_callback,
)
```

## Creating Callbacks

### `create_callback`

::: rattler_build.progress.create_callback

## Built-in Callbacks

### `SimpleProgressCallback`

::: rattler_build.progress.SimpleProgressCallback
    options:
        members:
            - on_download_start
            - on_download_progress
            - on_download_complete
            - on_build_step
            - on_log

### `RichProgressCallback`

::: rattler_build.progress.RichProgressCallback
    options:
        members:
            - __init__
            - __enter__
            - __exit__
            - on_download_start
            - on_download_progress
            - on_download_complete
            - on_build_step
            - on_log

## Protocol

### `ProgressCallback`

::: rattler_build.progress.ProgressCallback
    options:
        members:
            - on_download_start
            - on_download_progress
            - on_download_complete
            - on_build_step
            - on_log

## Events

### `DownloadStartEvent`

::: rattler_build.progress.DownloadStartEvent
    options:
        members:
            - url
            - total_bytes

### `DownloadProgressEvent`

::: rattler_build.progress.DownloadProgressEvent
    options:
        members:
            - url
            - bytes_downloaded
            - total_bytes

### `DownloadCompleteEvent`

::: rattler_build.progress.DownloadCompleteEvent
    options:
        members:
            - url

### `BuildStepEvent`

::: rattler_build.progress.BuildStepEvent
    options:
        members:
            - step_name
            - message

### `LogEvent`

::: rattler_build.progress.LogEvent
    options:
        members:
            - level
            - message
            - span
