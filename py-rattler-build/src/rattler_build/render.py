"""
Recipe rendering functionality for converting Stage0 to Stage1 recipes with variants.

This module provides the ability to render Stage0 recipes (parsed but unevaluated)
into Stage1 recipes (fully evaluated and ready to build) using variant configurations.
"""

from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional, Union

from rattler_build.stage0 import MultiOutputRecipe, SingleOutputRecipe

# Import for type hints only - avoid circular import
if TYPE_CHECKING:
    from rattler_build.tool_config import ToolConfiguration

# Type for context values - can be strings, numbers, bools, or lists
ContextValue = str | int | float | bool | list[str | int | float | bool]

# Try to import TypeAlias for better type hint support
try:
    from typing import TypeAlias
except ImportError:
    from typing import TypeAlias

if TYPE_CHECKING:
    from rattler_build.variant_config import VariantConfig

    # For type checking, use Any placeholders
    _RenderConfig = Any
    _RenderedVariant = Any
    _HashInfo = Any
    _PinSubpackageInfo = Any
    _render_recipe = Any
else:
    # At runtime, import from the Rust module
    from rattler_build import _rattler_build as _rb

    _render = _rb.render
    _RenderConfig = _render.RenderConfig
    _RenderedVariant = _render.RenderedVariant
    _HashInfo = _render.HashInfo
    _PinSubpackageInfo = _render.PinSubpackageInfo
    _render_recipe = _render.render_recipe


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

    def __init__(self, inner: _HashInfo):
        """Create a HashInfo from the Rust object."""
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
        >>> pins = variant.pin_subpackages()
        >>> for name, info in pins.items():
        ...     print(f"{name}: {info.version} (exact={info.exact})")
    """

    def __init__(self, inner: _PinSubpackageInfo):
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
        self._config = _RenderConfig(
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

    def __init__(self, inner: _RenderedVariant):
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
        return HashInfo(inner) if inner else None

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


RecipeInput: TypeAlias = str | SingleOutputRecipe | MultiOutputRecipe | Path


def render_recipe(
    recipe: RecipeInput | list[RecipeInput],
    variant_config: Union["VariantConfig", Path, str],
    render_config: RenderConfig | None = None,
) -> list[RenderedVariant]:
    """Render a Stage0 recipe with a variant configuration into Stage1 recipes.

    This function takes a parsed Stage0 recipe and evaluates all Jinja templates
    with different variant combinations to produce ready-to-build Stage1 recipes.

    Args:
        recipe: The Stage0 recipe to render (from stage0.Recipe.from_yaml())
        variant_config: The variant configuration (from variant_config.VariantConfig)
        render_config: Optional render configuration (defaults to current platform)

    Returns:
        List of RenderedVariant objects, one for each variant combination

    Example:
        >>> from rattler_build.stage0 import Recipe
        >>> from rattler_build.variant_config import VariantConfig
        >>> from rattler_build.render import render_recipe, RenderConfig
        >>>
        >>> # Parse stage0 recipe
        >>> recipe = Recipe.from_yaml('''
        ... package:
        ...   name: my-package
        ...   version: 1.0.0
        ... requirements:
        ...   host:
        ...     - python ${{ python }}
        ... ''')
        >>>
        >>> # Create variant config
        >>> variant_config = VariantConfig.from_yaml('''
        ... python:
        ...   - "3.9"
        ...   - "3.10"
        ...   - "3.11"
        ... ''')
        >>>
        >>> # Render with all variants
        >>> rendered = render_recipe(recipe, variant_config)
        >>> print(f"Generated {len(rendered)} variants")
        Generated 3 variants
    """
    from rattler_build.stage0 import Recipe
    from rattler_build.variant_config import VariantConfig as VC

    # Handle render_config parameter
    config_inner = render_config._config if render_config else None

    # Handle recipe parameter - convert str/Path to Recipe objects
    recipes_to_render: list[SingleOutputRecipe | MultiOutputRecipe] = []

    if isinstance(recipe, list):
        # Handle list of recipes
        for r in recipe:
            if isinstance(r, str | Path):
                parsed = Recipe.from_file(r)
                recipes_to_render.append(parsed)
            elif isinstance(r, SingleOutputRecipe | MultiOutputRecipe):
                recipes_to_render.append(r)
            else:
                raise TypeError(f"Unsupported recipe type in list: {type(r)}")
    elif isinstance(recipe, str | Path):
        # Parse single recipe from file/string
        if isinstance(recipe, Path):
            # Definitely a file path
            parsed = Recipe.from_file(recipe)
        elif recipe.endswith(".yaml") or recipe.endswith(".yml") or "/" in recipe or "\\" in recipe:
            # String that looks like a file path
            parsed = Recipe.from_file(recipe)
        else:
            # Treat as YAML string
            parsed = Recipe.from_yaml(recipe)
        recipes_to_render.append(parsed)
    elif isinstance(recipe, SingleOutputRecipe | MultiOutputRecipe):
        recipes_to_render.append(recipe)
    else:
        raise TypeError(f"Unsupported recipe type: {type(recipe)}")

    # Handle variant_config parameter - convert str/Path to VariantConfig
    if isinstance(variant_config, str | Path):
        if isinstance(variant_config, Path):
            variant_config = VC.from_file(variant_config)
        else:
            # Check if it's a file path or YAML string
            if (
                variant_config.endswith(".yaml")
                or variant_config.endswith(".yml")
                or "/" in variant_config
                or "\\" in variant_config
            ):
                variant_config = VC.from_file(variant_config)
            else:
                variant_config = VC.from_yaml(variant_config)
    elif not isinstance(variant_config, VC):
        raise TypeError(f"Unsupported variant_config type: {type(variant_config)}")

    # Now unwrap to get inner Rust objects
    variant_config_inner = variant_config._inner

    # Render all recipes and collect results
    all_rendered: list[RenderedVariant] = []
    for recipe_obj in recipes_to_render:
        recipe_inner = recipe_obj._wrapper
        rendered = _render_recipe(recipe_inner, variant_config_inner, config_inner)
        all_rendered.extend([RenderedVariant(r) for r in rendered])

    return all_rendered


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


__all__ = [
    "RenderConfig",
    "RenderedVariant",
    "HashInfo",
    "PinSubpackageInfo",
    "render_recipe",
    "build_rendered_variants",
]
