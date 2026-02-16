"""
Recipe rendering functionality for converting Stage0 to Stage1 recipes with variants.

This module provides the ability to render Stage0 recipes (parsed but unevaluated)
into Stage1 recipes (fully evaluated and ready to build) using variant configurations.
"""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING

from rattler_build import stage1
from rattler_build._rattler_build import build_rendered_variant_py
from rattler_build._rattler_build import render as _render
from rattler_build.build_result import BuildResult
from rattler_build.tool_config import PlatformConfig, ToolConfiguration

if TYPE_CHECKING:
    from rattler_build.progress import ProgressCallback

ContextValue = str | int | float | bool | list[str | int | float | bool]


class HashInfo:
    """
    Hash information for a rendered variant.

    This class wraps the Rust HashInfo type and provides convenient access
    to hash information computed during recipe rendering.

    Attributes:
        hash: The hash string (first 7 letters of the sha1sum)
        prefix: The hash prefix (e.g., 'py38' or 'np111')

    Example:
        ```python
        hash_info = variant.hash_info
        if hash_info:
            print(f"Hash: {hash_info.hash}")
            print(f"Prefix: {hash_info.prefix}")
        ```
    """

    def __init__(self, inner: _render.HashInfo) -> None:
        """Create a HashInfo from the Rust object (internal use only)."""
        self._inner = inner

    @property
    def hash(self) -> str:
        """Get the hash string (first 7 letters of sha1sum)."""
        return self._inner.hash

    @property
    def prefix(self) -> str:
        """Get the hash prefix (e.g., 'py38' or 'np111')."""
        return self._inner.prefix

    def __repr__(self) -> str:
        return repr(self._inner)

    def __str__(self) -> str:
        return f"HashInfo(hash={self.hash!r}, prefix={self.prefix!r})"


class PinSubpackageInfo:
    """
    Information about a pin_subpackage dependency.

    This class wraps the Rust PinSubpackageInfo type and provides information
    about packages pinned via the pin_subpackage() Jinja function.

    Attributes:
        name: The name of the pinned subpackage
        version: The version of the pinned subpackage
        build_string: The build string of the pinned subpackage (if known)
        exact: Whether this is an exact pin

    Example:
        ```python
        pins = variant.pin_subpackages
        for name, info in pins.items():
            print(f"{name}: {info.version} (exact={info.exact})")
        ```
    """

    def __init__(self, inner: _render.PinSubpackageInfo):
        """Create a PinSubpackageInfo from the Rust object."""
        self._inner = inner

    @property
    def name(self) -> str:
        """Get the package name."""
        return self._inner.name

    @property
    def version(self) -> str:
        """Get the package version."""
        return self._inner.version

    @property
    def build_string(self) -> str | None:
        """Get the build string if available."""
        return self._inner.build_string

    @property
    def exact(self) -> bool:
        """Check if this is an exact pin."""
        return self._inner.exact

    def __repr__(self) -> str:
        return repr(self._inner)

    def __str__(self) -> str:
        return (
            f"PinSubpackageInfo(name={self.name!r}, version={self.version!r}, "
            f"build_string={self.build_string!r}, exact={self.exact})"
        )


class RenderConfig:
    """Configuration for rendering recipes with variants.

    This class configures how recipes are rendered, including platform settings,
    experimental features, and additional Jinja context variables.

    The ``recipe_path`` is **not** set here — it is automatically injected from
    the :class:`~rattler_build.stage0.Stage0Recipe` during :meth:`render`.

    Args:
        platform: Platform configuration (target, build, host platforms, experimental flag)
        extra_context: Dictionary of extra context variables for Jinja rendering

    Example:
        ```python
        from rattler_build.tool_config import PlatformConfig

        platform = PlatformConfig(target_platform="linux-64")
        config = RenderConfig(
            platform=platform,
            extra_context={"custom_var": "value", "build_num": 42}
        )
        ```
    """

    platform: PlatformConfig | None

    def __init__(
        self,
        platform: PlatformConfig | None = None,
        extra_context: dict[str, ContextValue] | None = None,
    ):
        """Create a new render configuration."""
        self.platform = platform
        self._extra_context = extra_context
        self._config = _render.RenderConfig(
            target_platform=platform.target_platform if platform else None,
            build_platform=platform.build_platform if platform else None,
            host_platform=platform.host_platform if platform else None,
            experimental=platform.experimental if platform else False,
            recipe_path=None,
            extra_context=extra_context,
        )

    @staticmethod
    def _with_recipe_path(
        render_config: RenderConfig | None,
        recipe_path: Path,
    ) -> _render.RenderConfig:
        """Return a Rust RenderConfig with *recipe_path* injected.

        This is an internal helper used by :meth:`Stage0Recipe.render` to
        ensure the recipe path is always set without requiring the user to
        pass it explicitly.
        """
        if render_config is not None:
            platform = render_config.platform
            extra_context = render_config._extra_context
        else:
            platform = None
            extra_context = None

        return _render.RenderConfig(
            target_platform=platform.target_platform if platform else None,
            build_platform=platform.build_platform if platform else None,
            host_platform=platform.host_platform if platform else None,
            experimental=platform.experimental if platform else False,
            recipe_path=recipe_path,
            extra_context=extra_context,
        )

    def get_context(self, key: str) -> ContextValue | None:
        """Get an extra context variable.

        Args:
            key: Variable name

        Returns:
            The variable value, or None if not found
        """
        return self._config.get_context(key)

    def get_all_context(self) -> dict[str, ContextValue]:
        """Get all extra context variables as a dictionary."""
        return self._config.get_all_context()

    @property
    def target_platform(self) -> str:
        """Get the target platform."""
        return self._config.target_platform()

    @property
    def build_platform(self) -> str:
        """Get the build platform."""
        return self._config.build_platform()

    @property
    def host_platform(self) -> str:
        """Get the host platform."""
        return self._config.host_platform()

    @property
    def experimental(self) -> bool:
        """Get whether experimental features are enabled."""
        return self._config.experimental()

    def __repr__(self) -> str:
        return repr(self._config)


class RenderedVariant:
    """Result of rendering a recipe with a specific variant combination.

    Each RenderedVariant represents one specific variant of a recipe after
    all Jinja templates have been evaluated and variant values applied.

    The :attr:`recipe_path` is carried over from the
    :class:`~rattler_build.stage0.Stage0Recipe` that produced this variant
    and is used automatically by :meth:`run_build`.

    Attributes:
        variant: The variant combination used (variable name -> value)
        recipe: The rendered Stage1 recipe
        hash_info: Build string hash information
        pin_subpackages: Pin subpackage dependencies
        recipe_path: Path to the recipe file on disk

    Example:
        ```python
        for variant in rendered_variants:
            print(f"Package: {variant.recipe.package.name}")
            print(f"Variant: {variant.variant}")
            print(f"Build string: {variant.recipe.build.string}")
        ```
    """

    def __init__(self, inner: _render.RenderedVariant, recipe_path: Path):
        """Create a RenderedVariant from the Rust object."""
        self._inner = inner
        self._recipe_path = recipe_path

    @property
    def recipe_path(self) -> Path:
        """Get the path to the recipe file on disk."""
        return self._recipe_path

    @property
    def variant(self) -> dict[str, str]:
        """Get the variant combination used for this render.

        Returns:
            Dictionary mapping variable names to their values
        """
        return self._inner.variant()

    @property
    def recipe(self) -> stage1.Stage1Recipe:
        """Get the rendered Stage1 recipe.

        Returns:
            The fully evaluated Stage1 recipe ready for building
        """
        return self._inner.recipe()

    @property
    def hash_info(self) -> HashInfo | None:
        """Get hash info if available.

        Returns:
            HashInfo object with 'hash' and 'prefix' attributes, or None

        Example:
            ```python
            rendered = recipe.render(variant_config)[0]
            hash_info = rendered.hash_info
            if hash_info:
                print(f"Hash: {hash_info.hash}")
                print(f"Prefix: {hash_info.prefix}")
            ```
        """
        inner = self._inner.hash_info()
        return HashInfo(inner) if inner else None

    @property
    def pin_subpackages(self) -> dict[str, PinSubpackageInfo]:
        """Get pin_subpackage information.

        Returns:
            Dictionary mapping package names to PinSubpackageInfo objects

        Example:
            ```python
            rendered = recipe.render(variant_config)[0]
            for name, info in rendered.pin_subpackages.items():
                print(f"{name}: version={info.version}, exact={info.exact}")
            ```
        """
        inner_dict = self._inner.pin_subpackages()
        return {name: PinSubpackageInfo(info) for name, info in inner_dict.items()}

    def run_build(
        self,
        tool_config: ToolConfiguration | None = None,
        output_dir: str | Path = ".",
        channels: list[str] | None = None,
        progress_callback: ProgressCallback | None = None,
        no_build_id: bool = False,
        package_format: str | None = None,
        no_include_recipe: bool = False,
        debug: bool = False,
        exclude_newer: datetime | None = None,
    ) -> BuildResult:
        """Build this rendered variant.

        This method builds a single rendered variant directly without needing
        to go back through the Stage0 recipe.  The recipe path is taken from
        this variant automatically (set during :meth:`Stage0Recipe.render`).

        Args:
            tool_config: ToolConfiguration to use for the build. If None, uses defaults.
            output_dir: Directory to store the built package. Defaults to current directory.
            channels: List of channels to use for resolving dependencies. Defaults to ["conda-forge"].
            progress_callback: Optional progress callback for build events.
            no_build_id: Don't include build ID in output directory.
            package_format: Package format ("conda" or "tar.bz2").
            no_include_recipe: Don't include recipe in the output package.
            debug: Enable debug mode.
            exclude_newer: Exclude packages newer than this timestamp.

        Returns:
            BuildResult: Information about the built package including paths, metadata, and timing.

        Example:
            ```python
            from rattler_build import Stage0Recipe, VariantConfig

            recipe = Stage0Recipe.from_yaml(yaml_string)
            rendered = recipe.render(VariantConfig())
            # Build just the first variant
            result = rendered[0].run_build(output_dir="./output")
            print(f"Built package: {result.packages[0]}")
            ```
        """
        # Use default ToolConfiguration if not provided
        if tool_config is None:
            tool_config = ToolConfiguration()

        # Build this single variant
        rust_result = build_rendered_variant_py(
            rendered_variant=self._inner,
            tool_config=tool_config._inner,
            output_dir=Path(output_dir),
            channels=channels if channels is not None else ["conda-forge"],
            progress_callback=progress_callback,
            recipe_path=self._recipe_path,
            no_build_id=no_build_id,
            package_format=package_format,
            no_include_recipe=no_include_recipe,
            debug=debug,
            exclude_newer=exclude_newer,
        )

        # Convert Rust BuildResult to Python BuildResult
        return BuildResult._from_inner(rust_result)

    def __repr__(self) -> str:
        return repr(self._inner)


def build_rendered_variants(
    rendered_variants: list[RenderedVariant],
    *,
    tool_config: ToolConfiguration | None = None,
    output_dir: str | Path = ".",
    channels: list[str] | None = None,
    progress_callback: ProgressCallback | None = None,
    no_build_id: bool = False,
    package_format: str | None = None,
    no_include_recipe: bool = False,
    debug: bool = False,
    exclude_newer: datetime | None = None,
) -> list[BuildResult]:
    """Build multiple rendered variants.

    This is a convenience function for building multiple rendered variants
    in one call, useful when you want to build all variants from a recipe.

    Each variant's :attr:`~RenderedVariant.recipe_path` is used automatically.

    Args:
        rendered_variants: List of RenderedVariant objects to build
        tool_config: ToolConfiguration to use for the build. If None, uses defaults.
        output_dir: Directory to store the built packages. Defaults to current directory.
        channels: List of channels to use for resolving dependencies. Defaults to ["conda-forge"].
        progress_callback: Optional progress callback for build events.
        no_build_id: Don't include build ID in output directory.
        package_format: Package format ("conda" or "tar.bz2").
        no_include_recipe: Don't include recipe in the output package.
        debug: Enable debug mode.
        exclude_newer: Exclude packages newer than this timestamp.

    Returns:
        list[BuildResult]: List of build results, one per variant built.

    Example:
        ```python
        from rattler_build import Stage0Recipe, VariantConfig
        from rattler_build.render import build_rendered_variants

        # Parse and render recipe
        recipe = Stage0Recipe.from_yaml(yaml_string)
        variant_config = VariantConfig.from_yaml('''
        python:
          - "3.9"
          - "3.10"
          - "3.11"
        ''')
        rendered = recipe.render(variant_config)

        # Build all variants at once
        results = build_rendered_variants(rendered, output_dir="./output")
        for result in results:
            print(f"Built {result.name} {result.version} for {result.platform}")

        # Or build a subset
        results = build_rendered_variants(rendered[:2], output_dir="./output")
        ```
    """
    results = []
    for variant in rendered_variants:
        result = variant.run_build(
            tool_config=tool_config,
            output_dir=output_dir,
            channels=channels,
            progress_callback=progress_callback,
            no_build_id=no_build_id,
            package_format=package_format,
            no_include_recipe=no_include_recipe,
            debug=debug,
            exclude_newer=exclude_newer,
        )
        results.append(result)

    return results
