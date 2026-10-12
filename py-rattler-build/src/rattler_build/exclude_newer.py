"""Dependency cutoff policies for builds, package tests, and debug sessions."""

from datetime import datetime

from rattler_build._rattler_build import ExcludeNewer as _ExcludeNewer


class ExcludeNewer:
    """Exclude dependencies newer than global, package, or channel cutoffs.

    Args:
        cutoff: Global cutoff. If omitted, only package and channel cutoffs apply.
        packages: Cutoffs keyed by package name. ``None`` values exempt a package.
            Package cutoffs take precedence over channel and global cutoffs.
        channels: Cutoffs keyed by exact, absolute channel URL. ``None`` values
            exempt a channel. Channel cutoffs take precedence over the global cutoff.
        include_unknown_timestamp: Include packages without a timestamp when filtering.

    A policy without a cutoff or overrides does not enable filtering. Packages
    produced by the current build are exempt unless a package cutoff applies.

    Example:
        ```python
        from datetime import datetime, timezone
        from rattler_build import ExcludeNewer

        policy = ExcludeNewer(
            datetime(2024, 1, 1, tzinfo=timezone.utc),
            packages={"python": None},
            channels={"https://conda.anaconda.org/conda-forge/": None},
        )
        result = variant.run_build(exclude_newer=policy)
        ```
    """

    def __init__(
        self,
        cutoff: datetime | None = None,
        *,
        packages: dict[str, datetime | None] | None = None,
        channels: dict[str, datetime | None] | None = None,
        include_unknown_timestamp: bool = False,
    ):
        self._inner = _ExcludeNewer(
            cutoff,
            packages=packages,
            channels=channels,
            include_unknown_timestamp=include_unknown_timestamp,
        )
