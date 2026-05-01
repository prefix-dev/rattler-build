"""Debug session API for interactive build debugging.

The :class:`DebugSession` sets up a full build environment (resolves
dependencies, fetches sources, installs environments, creates build
script) **without** running the build. You can then iteratively run and
re-run the build script, inspecting output each time.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from rattler_build._rattler_build import debug as _debug

if TYPE_CHECKING:
    from rattler_build.progress import ProgressCallback
    from rattler_build.render import RenderedVariant
    from rattler_build.tool_config import ToolConfiguration


@dataclass(frozen=True)
class ScriptResult:
    """Result of running a build script.

    Non-zero ``exit_code`` is **not** an exception — it is expected during
    debugging.  Inspect ``stdout`` and ``stderr`` to diagnose the failure.
    """

    exit_code: int
    """Process exit code (0 = success)."""

    stdout: str
    """Captured standard output."""

    stderr: str
    """Captured standard error."""

    @property
    def success(self) -> bool:
        """Whether the script exited successfully."""
        return self.exit_code == 0


@dataclass(frozen=True)
class DebugPaths:
    """All paths associated with a debug session."""

    work_dir: Path
    """Work directory where the build script and sources live."""

    host_prefix: Path
    """Host prefix (where host dependencies are installed)."""

    build_prefix: Path
    """Build prefix (where build dependencies are installed)."""

    build_script: Path
    """Path to the build script (``conda_build.sh`` / ``conda_build.bat``)."""

    build_env_script: Path
    """Path to the environment script (``build_env.sh`` / ``build_env.bat``)."""

    build_dir: Path
    """Build directory (parent of work, host_env, build_env)."""

    output_dir: Path
    """Output directory where packages are written."""

    recipe_dir: Path
    """Recipe directory."""


class DebugSession:
    """Interactive debugging session for a conda recipe.

    Use :meth:`create` to set up the environment. Then call :meth:`run_script`
    to execute the build, inspect results, modify files, and re-run.

    Example::

        session = DebugSession.create(variant, channels=["conda-forge"])
        result = session.run_script(trace=True)
        print(result.stdout)
    """

    def __init__(self, inner: _debug.DebugSession) -> None:
        self._inner = inner

    @classmethod
    def create(
        cls,
        variant: RenderedVariant,
        *,
        tool_config: ToolConfiguration | None = None,
        output_dir: str | Path | None = None,
        channels: list[str] | None = None,
        no_build_id: bool = True,
        progress_callback: ProgressCallback | None = None,
    ) -> DebugSession:
        """Create a debug session from a rendered variant.

        This resolves dependencies, fetches sources, installs environments,
        and creates the build script — all without running the actual build.

        Parameters
        ----------
        variant:
            A rendered variant from ``recipe.render()``.
        tool_config:
            Optional tool configuration. Uses defaults if not provided.
        output_dir:
            Where to write build artifacts. A temporary directory is used if
            not specified.
        channels:
            Conda channels to use. Defaults to ``["conda-forge"]``.
        no_build_id:
            If ``True`` (default), omit the build ID from directory paths,
            giving stable paths across iterations.
        progress_callback:
            Optional callback for progress events during setup.

        Returns
        -------
        DebugSession
            A session with the environment fully set up and ready to run.
        """
        output_dir_path = Path(output_dir) if output_dir is not None else None
        tc = tool_config._inner if tool_config is not None else None
        inner = _debug.create_debug_session_py(
            rendered_variant=variant._inner,
            tool_config=tc,
            output_dir=output_dir_path,
            channels=channels,
            no_build_id=no_build_id,
            progress_callback=progress_callback,
            v3=variant._v3,
        )
        return cls(inner)

    @property
    def paths(self) -> DebugPaths:
        """All paths associated with this debug session."""
        return DebugPaths(
            work_dir=Path(self._inner.work_dir),
            host_prefix=Path(self._inner.host_prefix),
            build_prefix=Path(self._inner.build_prefix),
            build_script=Path(self._inner.build_script),
            build_env_script=Path(self._inner.build_env_script),
            build_dir=Path(self._inner.build_dir),
            output_dir=Path(self._inner.output_dir),
            recipe_dir=Path(self._inner.recipe_dir),
        )

    # Convenience accessors (match the old tutorial API)

    @property
    def work_dir(self) -> Path:
        """Work directory where the build script and sources live."""
        return Path(self._inner.work_dir)

    @property
    def host_prefix(self) -> Path:
        """Host prefix (where host dependencies are installed)."""
        return Path(self._inner.host_prefix)

    @property
    def build_prefix(self) -> Path:
        """Build prefix (where build dependencies are installed)."""
        return Path(self._inner.build_prefix)

    @property
    def build_script(self) -> Path:
        """Path to the build script."""
        return Path(self._inner.build_script)

    @property
    def output_dir(self) -> Path:
        """Output directory where packages are written."""
        return Path(self._inner.output_dir)

    @property
    def setup_log(self) -> list[str]:
        """Log messages captured during environment setup."""
        return self._inner.setup_log

    @property
    def log(self) -> list[str]:
        """Alias for :attr:`setup_log` (backwards compatibility)."""
        return self._inner.setup_log

    def run_script(self, *, trace: bool = False) -> ScriptResult:
        """Run the build script and capture output.

        Parameters
        ----------
        trace:
            If ``True``, run bash with ``-x`` flag to trace each command.

        Returns
        -------
        ScriptResult
            The exit code, stdout, and stderr from the build script.
        """
        exit_code, stdout, stderr = self._inner.run_script(trace=trace)
        return ScriptResult(exit_code=exit_code, stdout=stdout, stderr=stderr)

    # Alias for backwards compatibility with old tutorial
    run = run_script

    def add_packages(
        self,
        specs: list[str],
        *,
        environment: str = "host",
        channels: list[str] | None = None,
    ) -> list[str]:
        """Add packages to the host or build environment.

        Parameters
        ----------
        specs:
            Conda match specs (e.g. ``["numpy", "pandas>=2"]``).
        environment:
            ``"host"`` or ``"build"``.
        channels:
            Override channels for this operation.

        Returns
        -------
        list[str]
            The specs that were requested (confirmation).
        """
        return self._inner.add_packages(specs, environment=environment, channels=channels)

    def create_patch(
        self,
        name: str = "changes",
        *,
        output_dir: str | Path | None = None,
        overwrite: bool = False,
        exclude: list[str] | None = None,
        add: list[str] | None = None,
        include: list[str] | None = None,
        dry_run: bool = False,
    ) -> str:
        """Create a patch from changes in the work directory.

        Parameters
        ----------
        name:
            Patch file name (without extension).
        output_dir:
            Where to write the patch file. Defaults to the recipe directory.
        overwrite:
            Overwrite existing patch file.
        exclude:
            Glob patterns to exclude from the patch.
        add:
            Glob patterns for untracked files to include.
        include:
            Glob patterns to restrict the patch to.
        dry_run:
            If ``True``, print the patch without writing it.

        Returns
        -------
        str
            Empty string on success.
        """
        output_dir_path = Path(output_dir) if output_dir is not None else None
        return self._inner.create_patch(
            name=name,
            output_dir=output_dir_path,
            overwrite=overwrite,
            exclude=exclude,
            add=add,
            include=include,
            dry_run=dry_run,
        )

    def read_build_script(self) -> str:
        """Read and return the build script contents."""
        return self._inner.read_build_script()

    def __repr__(self) -> str:
        return f"DebugSession(work_dir='{self.work_dir}')"
