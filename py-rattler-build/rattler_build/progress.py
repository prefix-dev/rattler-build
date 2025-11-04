"""
Progress reporting and callbacks for rattler-build.

This module provides base classes and implementations for progress reporting
during recipe rendering and building. You can use the built-in implementations
(RichProgressCallback, TqdmProgressCallback) or create your own by subclassing
ProgressCallback.
"""

from typing import Protocol, runtime_checkable

from rattler_build.rattler_build import (
    DownloadStartEvent,
    DownloadProgressEvent,
    DownloadCompleteEvent,
    BuildStepEvent,
    LogEvent,
)


__all__ = [
    "ProgressCallback",
    "DownloadStartEvent",
    "DownloadProgressEvent",
    "DownloadCompleteEvent",
    "BuildStepEvent",
    "LogEvent",
    "SimpleProgressCallback",
    "RichProgressCallback",
]


@runtime_checkable
class ProgressCallback(Protocol):
    """Protocol for progress callbacks.

    Implement this protocol to receive progress updates during builds.
    All methods are optional - only implement the ones you need.

    Example:
        ```python
        class MyCallback(ProgressCallback):
            def on_download_progress(self, event: DownloadProgressEvent):
                percent = event.bytes_downloaded / event.total_bytes * 100
                print(f"Downloaded {percent:.1f}%")

            def on_build_step(self, event: BuildStepEvent):
                print(f"[{event.step_name}] {event.message}")
        ```
    """

    def on_download_start(self, event: DownloadStartEvent) -> None:
        """Called when a download starts.

        Args:
            event: Event containing download URL and expected total bytes
        """
        ...

    def on_download_progress(self, event: DownloadProgressEvent) -> None:
        """Called periodically during download to report progress.

        Args:
            event: Event containing bytes downloaded and total bytes
        """
        ...

    def on_download_complete(self, event: DownloadCompleteEvent) -> None:
        """Called when a download completes successfully.

        Args:
            event: Event containing the download URL
        """
        ...

    def on_build_step(self, event: BuildStepEvent) -> None:
        """Called when a new build step begins.

        Args:
            event: Event containing step name and message
        """
        ...

    def on_log(self, event: LogEvent) -> None:
        """Called for log messages.

        Args:
            event: Event containing log level, message, and optional span
        """
        ...


class SimpleProgressCallback:
    """Simple console-based progress callback.

    Prints progress updates to the console with simple formatting.

    Example:
        ```python
        from rattler_build import Recipe, VariantConfig
        from rattler_build.progress import SimpleProgressCallback

        recipe = Recipe.from_file("recipe.yaml")
        rendered = recipe.render(VariantConfig())

        callback = SimpleProgressCallback()
        # Use callback in build (to be implemented)
        ```
    """

    def on_download_start(self, event: DownloadStartEvent) -> None:
        """Print download start message."""
        if event.total_bytes:
            print(f"📥 Downloading {event.url} ({event.total_bytes / 1024 / 1024:.1f} MB)")
        else:
            print(f"📥 Downloading {event.url}")

    def on_download_progress(self, event: DownloadProgressEvent) -> None:
        """Print download progress (only at 25% intervals to avoid spam)."""
        if event.total_bytes:
            percent = (event.bytes_downloaded / event.total_bytes) * 100
            if int(percent) % 25 == 0 and percent > 0:
                print(f"   {percent:.0f}% complete")

    def on_download_complete(self, event: DownloadCompleteEvent) -> None:
        """Print download complete message."""
        print(f"✅ Downloaded {event.url}")

    def on_build_step(self, event: BuildStepEvent) -> None:
        """Print build step message."""
        print(f"🔨 [{event.step_name}] {event.message}")

    def on_log(self, event: LogEvent) -> None:
        """Print log message with appropriate prefix."""
        prefix = {
            "error": "❌",
            "warn": "⚠️ ",
            "info": "ℹ️ ",
        }.get(event.level, "  ")
        span_str = f" [{event.span}]" if event.span else ""
        print(f"{prefix}{span_str} {event.message}")


class RichProgressCallback:
    """Rich-based progress callback with beautiful terminal output.

    Automatically creates progress bars for long-running operations by parsing
    log messages. Shows spinners for operations and bars for downloads.

    Requires the 'rich' library to be installed:
        pip install rich

    Example:
        ```python
        from rattler_build import Recipe, VariantConfig
        from rattler_build.progress import RichProgressCallback

        recipe = Recipe.from_file("recipe.yaml")
        rendered = recipe.render(VariantConfig())

        with RichProgressCallback() as callback:
            # Use callback in build (to be implemented)
            pass
        ```
    """

    def __init__(self, show_logs: bool = True, show_details: bool = False):
        """Initialize the Rich progress callback.

        Args:
            show_logs: Whether to display all log messages (default: True - recommended for debugging)
            show_details: Whether to show detailed logs like index operations (default: False)
        """
        try:
            from rich.progress import (
                Progress,
                SpinnerColumn,
                BarColumn,
                TextColumn,
                TimeElapsedColumn,
            )
            from rich.console import Console
        except ImportError:
            raise ImportError("Rich library is required for RichProgressCallback. " "Install it with: pip install rich")

        self.show_logs = show_logs
        self.show_details = show_details
        self.console = Console()

        # Create progress for operations
        self.progress = Progress(
            SpinnerColumn(),
            TextColumn("[bold blue]{task.description}"),
            BarColumn(complete_style="green", finished_style="bold green"),
            TextColumn("[progress.percentage]{task.percentage:>3.0f}%"),
            TimeElapsedColumn(),
            console=self.console,
        )

        self.tasks = {}  # Download tasks
        self.operation_tasks = {}  # Operation tasks (resolving, building, etc.)
        self.current_operation = None

    def __enter__(self):
        """Context manager entry."""
        self.progress.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Context manager exit."""
        self.progress.stop()

    def on_download_start(self, event: DownloadStartEvent) -> None:
        """Create a progress bar for the download."""
        task_id = self.progress.add_task(
            f"Downloading {self._shorten_url(event.url)}",
            total=event.total_bytes,
        )
        self.tasks[event.url] = task_id

    def on_download_progress(self, event: DownloadProgressEvent) -> None:
        """Update the download progress bar."""
        task_id = self.tasks.get(event.url)
        if task_id is not None:
            self.progress.update(task_id, completed=event.bytes_downloaded)

    def on_download_complete(self, event: DownloadCompleteEvent) -> None:
        """Mark the download as complete."""
        task_id = self.tasks.get(event.url)
        if task_id is not None:
            self.progress.update(task_id, completed=True)
            del self.tasks[event.url]

    def on_build_step(self, event: BuildStepEvent) -> None:
        """Update or create a build step task."""
        if self.step_task is not None:
            self.progress.remove_task(self.step_task)

        self.step_task = self.progress.add_task(
            f"[cyan]{event.step_name}[/cyan]: {event.message}",
            total=None,  # Indeterminate progress
        )

    def on_log(self, event: LogEvent) -> None:
        """Parse log messages and create/update progress bars."""

        msg = event.message
        span = event.span or ""

        # Skip noisy index operations unless show_details is True
        if not self.show_details and (
            "index_subdir" in span or "Adding 0 packages" in msg or "Writing repodata" in msg
        ):
            return

        # Detect operation starts and create progress indicators
        if "Starting build of" in msg:
            self._complete_operation()
            self.current_operation = self.progress.add_task("🔨 Building package", total=100)
            self.progress.update(self.current_operation, advance=10)

        elif "Fetching source code" in span:
            self._complete_operation()
            self.current_operation = self.progress.add_task("📥 Fetching sources", total=100)
            if "No sources" in msg:
                self.progress.update(self.current_operation, completed=100)
            else:
                self.progress.update(self.current_operation, advance=50)

        elif "Resolving environments" in span:
            if self.current_operation is None or "Fetching" not in str(
                self.progress.tasks[self.current_operation].description
            ):
                self._complete_operation()
                self.current_operation = self.progress.add_task("🔍 Resolving dependencies", total=100)
            # Advance progress as we see different stages
            if "Platform:" in msg:
                self.progress.update(self.current_operation, advance=20)
            elif "Specs:" in msg:
                self.progress.update(self.current_operation, advance=20)

        elif "get_or_create_subdir" in span and "sharded repodata" in msg:
            if self.current_operation:
                self.progress.update(self.current_operation, advance=5)

        elif "Running build for" in span:
            # Only create the task once for the entire build script phase
            if self.current_operation is None or "⚙️  Running build script" not in str(
                self.progress.tasks[self.current_operation].description
            ):
                self._complete_operation()
                self.current_operation = self.progress.add_task("⚙️  Running build script", total=100)

            # Update progress based on environment updates
            if "Successfully updated the build environment" in msg:
                self.progress.update(self.current_operation, advance=50)
            elif "Successfully updated the host environment" in msg:
                self.progress.update(self.current_operation, completed=100)

        elif "Packaging new files" in span:
            # Only create the packaging task once, not for every log message
            if self.current_operation is None or "📦 Packaging" not in str(
                self.progress.tasks[self.current_operation].description
            ):
                self._complete_operation()
                self.current_operation = self.progress.add_task("📦 Packaging", total=100)

            # Update progress based on packaging steps
            if "Copying done" in msg:
                self.progress.update(self.current_operation, advance=30)
            elif "Post-processing done" in msg:
                self.progress.update(self.current_operation, advance=30)
            elif "Writing test files" in msg:
                self.progress.update(self.current_operation, advance=10)
            elif "Writing metadata" in msg:
                self.progress.update(self.current_operation, advance=15)
            elif "Copying license" in msg:
                self.progress.update(self.current_operation, advance=10)
            elif "Copying recipe" in msg:
                self.progress.update(self.current_operation, advance=5)

        # Show important messages or warnings/errors
        if event.level in ("error", "warn") or self.show_logs:
            style_map = {
                "error": "bold red",
                "warn": "bold yellow",
                "info": "cyan",
            }
            style = style_map.get(event.level, "")

            # Format with span if available
            if span and event.level == "info":
                formatted_msg = f"[dim]│[/dim] [{style}]{span}[/{style}] {msg}"
            elif event.level in ("error", "warn"):
                prefix = "❌" if event.level == "error" else "⚠️"
                formatted_msg = f"[dim]│[/dim] {prefix} [{style}]{msg}[/{style}]"
            else:
                formatted_msg = f"[dim]│[/dim] {msg}"

            if event.level in ("error", "warn") or (self.show_logs and event.level == "info"):
                self.console.print(formatted_msg)

    def _complete_operation(self):
        """Complete the current operation task."""
        if self.current_operation is not None:
            self.progress.update(self.current_operation, completed=100)
            self.current_operation = None

    @staticmethod
    def _shorten_url(url: str, max_len: int = 50) -> str:
        """Shorten a URL for display."""
        if len(url) <= max_len:
            return url
        return url[: max_len - 3] + "..."


# Create a simple default callback for convenience
default_callback = SimpleProgressCallback()


def create_callback(style: str = "simple", **kwargs) -> ProgressCallback:
    """Create a progress callback of the specified style.

    Args:
        style: Style of callback - "simple", "rich", or "none"
        **kwargs: Additional arguments passed to the callback constructor

    Returns:
        A progress callback instance

    Example:
        ```python
        # Simple console output
        callback = create_callback("simple")

        # Rich terminal output
        callback = create_callback("rich", show_logs=True)

        # No output
        callback = create_callback("none")
        ```
    """
    if style == "simple":
        return SimpleProgressCallback()
    elif style == "rich":
        return RichProgressCallback(**kwargs)
    elif style == "none":
        # Empty callback that does nothing
        class NoOpCallback:
            def on_download_start(self, event):
                pass

            def on_download_progress(self, event):
                pass

            def on_download_complete(self, event):
                pass

            def on_build_step(self, event):
                pass

            def on_log(self, event):
                pass

        return NoOpCallback()
    else:
        raise ValueError(f"Unknown callback style: {style}")
