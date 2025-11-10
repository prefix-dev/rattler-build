"""
Recipe rendering functionality for converting Stage0 to Stage1 recipes with variants.

This module provides the ability to render Stage0 recipes (parsed but unevaluated)
into Stage1 recipes (fully evaluated and ready to build) using variant configurations.
"""

from pathlib import Path
from typing import Any, Optional

from rattler_build._rattler_build import render as _render
from rattler_build.tool_config import ToolConfiguration

__all__ = [
    "RenderConfig",
    "RenderedVariant",
    "HashInfo",
    "PinSubpackageInfo",
    "build_rendered_variants",
]

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
        >>> hash_info = variant.hash_info()
        >>> if hash_info:
        ...     print(f"Hash: {hash_info.hash}")
        ...     print(f"Prefix: {hash_info.prefix}")
    """

    _inner: _render.HashInfo

    @classmethod
    def _from_inner(cls, inner: _render.HashInfo) -> "HashInfo":
        """Create a HashInfo from the Rust object (internal use only)."""
        instance = cls.__new__(cls)
        instance._inner = inner
        return instance

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
        >>> pins = variant.pin_subpackages()
        >>> for name, info in pins.items():
        ...     print(f"{name}: {info.version} (exact={info.exact})")
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

    Args:
        target_platform: Target platform (e.g., "linux-64", "osx-arm64")
        build_platform: Build platform (where the build runs)
        host_platform: Host platform (for cross-compilation)
        experimental: Enable experimental features
        recipe_path: Path to the recipe file (for relative path resolution)
        extra_context: Dictionary of extra context variables for Jinja rendering

    Example:
        >>> config = RenderConfig(
        ...     target_platform="linux-64",
        ...     experimental=True,
        ...     extra_context={"custom_var": "value", "build_num": 42}
        ... )
    """

    def __init__(
        self,
        target_platform: str | None = None,
        build_platform: str | None = None,
        host_platform: str | None = None,
        experimental: bool = False,
        recipe_path: str | None = None,
        extra_context: dict[str, ContextValue] | None = None,
    ):
        """Create a new render configuration."""
        self._config = _render.RenderConfig(
            target_platform=target_platform,
            build_platform=build_platform,
            host_platform=host_platform,
            experimental=experimental,
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

    @property
    def recipe_path(self) -> str | None:
        """Get the recipe path."""
        return self._config.recipe_path()

    def __repr__(self) -> str:
        return repr(self._config)


class RenderedVariant:
    """Result of rendering a recipe with a specific variant combination.

    Each RenderedVariant represents one specific variant of a recipe after
    all Jinja templates have been evaluated and variant values applied.

    Attributes:
        variant: The variant combination used (variable name -> value)
        recipe: The rendered Stage1 recipe
        hash_info: Build string hash information
        pin_subpackages: Pin subpackage dependencies

    Example:
        >>> for variant in rendered_variants:
        ...     print(f"Package: {variant.recipe().package().name()}")
        ...     print(f"Variant: {variant.variant()}")
        ...     print(f"Build string: {variant.recipe().build().string()}")
    """

    def __init__(self, inner: _render.RenderedVariant):
        """Create a RenderedVariant from the Rust object."""
        self._inner = inner

    def variant(self) -> dict[str, str]:
        """Get the variant combination used for this render.

        Returns:
            Dictionary mapping variable names to their values
        """
        return self._inner.variant()

    def recipe(self) -> Any:  # Returns Stage1Recipe
        """Get the rendered Stage1 recipe.

        Returns:
            The fully evaluated Stage1 recipe ready for building
        """
        return self._inner.recipe()

    def hash_info(self) -> HashInfo | None:
        """Get hash info if available.

        Returns:
            HashInfo object with 'hash' and 'prefix' attributes, or None

        Example:
            >>> rendered = render_recipe(recipe, variant_config)[0]
            >>> hash_info = rendered.hash_info()
            >>> if hash_info:
            ...     print(f"Hash: {hash_info.hash}")
            ...     print(f"Prefix: {hash_info.prefix}")
        """
        inner = self._inner.hash_info()
        return HashInfo._from_inner(inner) if inner else None

    def pin_subpackages(self) -> dict[str, PinSubpackageInfo]:
        """Get pin_subpackage information.

        Returns:
            Dictionary mapping package names to PinSubpackageInfo objects

        Example:
            >>> rendered = render_recipe(recipe, variant_config)[0]
            >>> for name, info in rendered.pin_subpackages().items():
            ...     print(f"{name}: version={info.version}, exact={info.exact}")
        """
        inner_dict = self._inner.pin_subpackages()
        return {name: PinSubpackageInfo(info) for name, info in inner_dict.items()}

    def run_build(
        self,
        tool_config: Optional["ToolConfiguration"] = None,
        output_dir: str | Path | None = None,
        channel: list[str] | None = None,
        progress_callback: Any | None = None,
        recipe_path: str | Path | None = None,
        **kwargs: Any,
    ) -> None:
        """Build this rendered variant.

        This method builds a single rendered variant directly without needing
        to go back through the Stage0 recipe.

        Args:
            tool_config: Optional ToolConfiguration to use for the build.
            output_dir: Directory to store the built package. Defaults to current directory.
            channel: List of channels to use for resolving dependencies.
            progress_callback: Optional progress callback for build events (e.g., RichProgressCallback or SimpleProgressCallback).
            recipe_path: Path to the recipe file (for copying license files, etc.). Defaults to None.
            **kwargs: Additional arguments passed to build (e.g., keep_build, test, etc.)

        Example:
            >>> from rattler_build.stage0 import Recipe
            >>> from rattler_build.variant_config import VariantConfig
            >>> from rattler_build.render import render_recipe
            >>>
            >>> recipe = Recipe.from_yaml(yaml_string)
            >>> rendered = render_recipe(recipe, VariantConfig())
            >>> # Build just the first variant
            >>> rendered[0].run_build(output_dir="./output")
        """
        from rattler_build import _rattler_build as _rb

        # Extract the inner ToolConfiguration if provided
        tool_config_inner = tool_config._inner if tool_config else None

        # Build this single variant
        _rb.build_from_rendered_variants_py(
            rendered_variants=[self._inner],
            tool_config=tool_config_inner,
            output_dir=Path(output_dir) if output_dir else None,
            channel=channel,
            progress_callback=progress_callback,
            recipe_path=Path(recipe_path) if recipe_path else None,
            **kwargs,
        )

    def __repr__(self) -> str:
        return repr(self._inner)


def build_rendered_variants(
    rendered_variants: list[RenderedVariant],
    tool_config: Optional["ToolConfiguration"] = None,
    output_dir: str | Path | None = None,
    channel: list[str] | None = None,
    progress_callback: Any | None = None,
    recipe_path: str | Path | None = None,
    **kwargs: Any,
) -> None:
    """Build multiple rendered variants.

    This is a convenience function for building multiple rendered variants
    in one call, useful when you want to build all variants from a recipe.

    Args:
        rendered_variants: List of RenderedVariant objects to build
        tool_config: Optional ToolConfiguration to use for the build.
        output_dir: Directory to store the built packages. Defaults to current directory.
        channel: List of channels to use for resolving dependencies.
        progress_callback: Optional progress callback for build events (e.g., RichProgressCallback or SimpleProgressCallback).
        recipe_path: Path to the recipe file (for copying license files, etc.). Defaults to None.
        **kwargs: Additional arguments passed to build (e.g., keep_build, test, etc.)

    Example:
        >>> from rattler_build.stage0 import Recipe
        >>> from rattler_build.variant_config import VariantConfig
        >>> from rattler_build.render import render_recipe, build_rendered_variants
        >>>
        >>> # Parse and render recipe
        >>> recipe = Recipe.from_yaml(yaml_string)
        >>> variant_config = VariantConfig.from_yaml('''
        ... python:
        ...   - "3.9"
        ...   - "3.10"
        ...   - "3.11"
        ... ''')
        >>> rendered = render_recipe(recipe, variant_config)
        >>>
        >>> # Build all variants at once
        >>> build_rendered_variants(rendered, output_dir="./output")
        >>>
        >>> # Or build a subset
        >>> build_rendered_variants(rendered[:2], output_dir="./output")
    """
    from rattler_build import _rattler_build as _rb

    # Extract the inner ToolConfiguration if provided
    tool_config_inner = tool_config._inner if tool_config else None

    # Build all variants
    _rb.build_from_rendered_variants_py(
        rendered_variants=[v._inner for v in rendered_variants],
        tool_config=tool_config_inner,
        output_dir=Path(output_dir) if output_dir else None,
        channel=channel,
        progress_callback=progress_callback,
        recipe_path=Path(recipe_path) if recipe_path else None,
        **kwargs,
    )
