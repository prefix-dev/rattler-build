"""
Stage0 - Parsed recipe types before evaluation.

This module provides Python bindings for rattler-build's stage0 types,
which represent the parsed YAML recipe before Jinja template evaluation
and conditional resolution.
"""

from pathlib import Path
from typing import Any, Dict, List, Optional, Union, TYPE_CHECKING

if TYPE_CHECKING:
    from rattler_build.tool_config import ToolConfiguration

    # For type checking, use Any as placeholder since we don't have stubs
    _Stage0Recipe = Any
    _SingleOutputRecipe = Any
    _MultiOutputRecipe = Any
    _Stage0Package = Any
    _Stage0PackageMetadata = Any
    _Stage0RecipeMetadata = Any
    _Stage0Build = Any
    _Stage0Requirements = Any
    _Stage0About = Any
    _Stage0PackageOutput = Any
    _Stage0StagingOutput = Any
else:
    # At runtime, import the Rust submodule
    from . import rattler_build as _rb

    # Get the stage0 submodule
    _stage0 = _rb.stage0

    # Import classes from the stage0 submodule
    _Stage0Recipe = _stage0.Stage0Recipe
    _SingleOutputRecipe = _stage0.SingleOutputRecipe
    _MultiOutputRecipe = _stage0.MultiOutputRecipe
    _Stage0Package = _stage0.Stage0Package
    _Stage0PackageMetadata = _stage0.Stage0PackageMetadata
    _Stage0RecipeMetadata = _stage0.Stage0RecipeMetadata
    _Stage0Build = _stage0.Stage0Build
    _Stage0Requirements = _stage0.Stage0Requirements
    _Stage0About = _stage0.Stage0About
    _Stage0PackageOutput = _stage0.Stage0PackageOutput
    _Stage0StagingOutput = _stage0.Stage0StagingOutput

__all__ = [
    "Recipe",
    "SingleOutputRecipe",
    "MultiOutputRecipe",
    "Package",
    "RecipeMetadata",
    "Build",
    "Requirements",
    "About",
    "PackageOutput",
    "StagingOutput",
]


class Recipe:
    """
    A parsed conda recipe (stage0).

    This can be either a single-output or multi-output recipe.

    Example:
        >>> recipe = Recipe.from_yaml(yaml_string)
        >>> if recipe.is_single_output():
        ...     single = recipe.as_single_output()
        ...     print(single.package.name)
    """

    def __init__(self, inner: _Stage0Recipe):
        self._inner = inner

    @classmethod
    def from_yaml(cls, yaml: str) -> Union["SingleOutputRecipe", "MultiOutputRecipe"]:
        """
        Parse a recipe from YAML string.

        Returns the appropriate type: SingleOutputRecipe or MultiOutputRecipe.
        """
        wrapper = _Stage0Recipe.from_yaml(yaml)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper)

    @classmethod
    def from_file(cls, path: Union[str, Path]) -> Union["SingleOutputRecipe", "MultiOutputRecipe"]:
        """
        Parse a recipe from a YAML file.

        Returns the appropriate type: SingleOutputRecipe or MultiOutputRecipe.
        """
        with open(path, "r", encoding="utf-8") as f:
            return cls.from_yaml(f.read())

    @classmethod
    def from_dict(cls, recipe_dict: Dict[str, Any]) -> Union["SingleOutputRecipe", "MultiOutputRecipe"]:
        """
        Create a recipe from a Python dictionary.

        This method validates the dictionary structure and provides detailed error
        messages if the structure is invalid or types don't match.

        Args:
            recipe_dict: Dictionary containing recipe data (must match recipe schema)

        Returns:
            SingleOutputRecipe or MultiOutputRecipe depending on the recipe type

        Raises:
            PyRecipeParseError: If the dictionary structure is invalid or types don't match

        Example:
            >>> recipe_dict = {
            ...     "package": {
            ...         "name": "my-package",
            ...         "version": "1.0.0"
            ...     },
            ...     "build": {
            ...         "number": 0
            ...     }
            ... }
            >>> recipe = Recipe.from_dict(recipe_dict)
        """
        wrapper = _Stage0Recipe.from_dict(recipe_dict)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper)

    def is_single_output(self) -> bool:
        """Check if this is a single output recipe."""
        return self._inner.is_single_output()

    def is_multi_output(self) -> bool:
        """Check if this is a multi output recipe."""
        return self._inner.is_multi_output()

    def as_single_output(self) -> Optional["SingleOutputRecipe"]:
        """Get as a single output recipe (None if multi-output)."""
        inner = self._inner.as_single_output()
        return SingleOutputRecipe(inner) if inner else None

    def as_multi_output(self) -> Optional["MultiOutputRecipe"]:
        """Get as a multi output recipe (None if single-output)."""
        inner = self._inner.as_multi_output()
        return MultiOutputRecipe(inner) if inner else None

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class SingleOutputRecipe:
    """A single-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _SingleOutputRecipe, wrapper: Any = None):
        self._inner = inner
        # Keep reference to the original Rust Stage0Recipe wrapper for render()
        self._wrapper = wrapper

    @property
    def schema_version(self) -> int:
        """Get the schema version."""
        return self._inner.schema_version

    @property
    def context(self) -> Dict[str, Any]:
        """Get the context variables as a dictionary."""
        return self._inner.context

    @property
    def package(self) -> "Package":
        """Get the package metadata."""
        return Package(self._inner.package)

    @property
    def build(self) -> "Build":
        """Get the build configuration."""
        return Build(self._inner.build)

    @property
    def requirements(self) -> "Requirements":
        """Get the requirements."""
        return Requirements(self._inner.requirements)

    @property
    def about(self) -> "About":
        """Get the about metadata."""
        return About(self._inner.about)

    def render(self, variant_config: Any = None, render_config: Any = None) -> List[Any]:
        """
        Render this recipe with variant configuration.

        This is a convenience method that calls render.render_recipe() internally.
        Always returns a list of RenderedVariant objects.

        Args:
            variant_config: Optional VariantConfig to use. If None, creates an empty config.
            render_config: Optional RenderConfig to use. If None, uses default config.

        Returns:
            List of RenderedVariant objects (one for each variant combination)

        Example:
            >>> recipe = Recipe.from_yaml(yaml_string)
            >>> variants = recipe.render(variant_config)
            >>> for variant in variants:
            ...     print(variant.recipe().package.name)
        """
        # Import here to avoid circular dependency
        from . import render as render_module
        from . import variant_config as vc_module

        # Create empty variant config if not provided
        if variant_config is None:
            variant_config = vc_module.VariantConfig()

        # Pass self (the Python wrapper) to render_recipe, not the raw Rust object
        return render_module.render_recipe(self, variant_config, render_config)

    def run_build(
        self,
        variant_config: Any = None,
        tool_config: Optional["ToolConfiguration"] = None,
        output_dir: Union[str, Path, None] = None,
        channel: Optional[List[str]] = None,
        **kwargs: Any,
    ) -> None:
        """
        Build this recipe.

        This method renders the recipe with variants and then builds the rendered outputs
        directly without writing temporary files.

        Args:
            variant_config: Optional VariantConfig to use for building variants.
            tool_config: Optional ToolConfiguration to use for the build. If provided, individual
                        parameters like keep_build, test, etc. will be ignored.
            output_dir: Directory to store the built packages. Defaults to current directory.
            channel: List of channels to use for resolving dependencies.
            **kwargs: Additional arguments passed to build (e.g., keep_build, test, etc.)
                     These are ignored if tool_config is provided.

        Example:
            >>> recipe = Recipe.from_yaml(yaml_string)
            >>> recipe.run_build(output_dir="./output")

            >>> # Or with custom tool configuration
            >>> from rattler_build import ToolConfiguration
            >>> config = ToolConfiguration(keep_build=True, test_strategy="native")
            >>> recipe.run_build(tool_config=config, output_dir="./output")
        """
        from . import rattler_build as _rb

        # Render the recipe to get Stage1 variants
        rendered_variants = self.render(variant_config)

        # Extract the inner ToolConfiguration if provided
        tool_config_inner = tool_config._inner if tool_config else None

        # Build from the rendered variants
        _rb.build_from_rendered_variants_py(
            rendered_variants=[v._inner for v in rendered_variants],
            tool_config=tool_config_inner,
            output_dir=Path(output_dir) if output_dir else None,
            channel=channel,
            **kwargs,
        )

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class MultiOutputRecipe:
    """A multi-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _MultiOutputRecipe, wrapper: Any = None):
        self._inner = inner
        # Keep reference to the original Rust Stage0Recipe wrapper for render()
        self._wrapper = wrapper

    @property
    def schema_version(self) -> int:
        """Get the schema version."""
        return self._inner.schema_version

    @property
    def context(self) -> Dict[str, Any]:
        """Get the context variables as a dictionary."""
        return self._inner.context

    @property
    def recipe(self) -> "RecipeMetadata":
        """Get the top-level recipe metadata."""
        return RecipeMetadata(self._inner.recipe)

    @property
    def build(self) -> "Build":
        """Get the top-level build configuration."""
        return Build(self._inner.build)

    @property
    def about(self) -> "About":
        """Get the top-level about metadata."""
        return About(self._inner.about)

    @property
    def outputs(self) -> List[Union["PackageOutput", "StagingOutput"]]:
        """Get all outputs (package and staging)."""
        result: List[Union["PackageOutput", "StagingOutput"]] = []
        for output in self._inner.outputs:
            if isinstance(output, _Stage0PackageOutput):  # type: ignore[misc]
                result.append(PackageOutput(output))
            elif isinstance(output, _Stage0StagingOutput):  # type: ignore[misc]
                result.append(StagingOutput(output))
        return result

    def render(self, variant_config: Any = None, render_config: Any = None) -> List[Any]:
        """
        Render this recipe with variant configuration.

        This is a convenience method that calls render.render_recipe() internally.
        Always returns a list of RenderedVariant objects.

        Args:
            variant_config: Optional VariantConfig to use. If None, creates an empty config.
            render_config: Optional RenderConfig to use. If None, uses default config.

        Returns:
            List of RenderedVariant objects (one for each variant combination and output)

        Example:
            >>> recipe = Recipe.from_yaml(yaml_string)
            >>> variants = recipe.render(variant_config)
            >>> for variant in variants:
            ...     print(variant.recipe().package.name)
        """
        # Import here to avoid circular dependency
        from . import render as render_module
        from . import variant_config as vc_module

        # Create empty variant config if not provided
        if variant_config is None:
            variant_config = vc_module.VariantConfig()

        # Pass self (the Python wrapper) to render_recipe, not the raw Rust object
        return render_module.render_recipe(self, variant_config, render_config)

    def run_build(
        self,
        variant_config: Any = None,
        tool_config: Optional["ToolConfiguration"] = None,
        output_dir: Union[str, Path, None] = None,
        channel: Optional[List[str]] = None,
        **kwargs: Any,
    ) -> None:
        """
        Build this multi-output recipe.

        This method renders the recipe with variants and then builds the rendered outputs
        directly without writing temporary files.

        Args:
            variant_config: Optional VariantConfig to use for building variants.
            tool_config: Optional ToolConfiguration to use for the build. If provided, individual
                        parameters like keep_build, test, etc. will be ignored.
            output_dir: Directory to store the built packages. Defaults to current directory.
            channel: List of channels to use for resolving dependencies.
            **kwargs: Additional arguments passed to build (e.g., keep_build, test, etc.)
                     These are ignored if tool_config is provided.

        Example:
            >>> recipe = Recipe.from_yaml(yaml_string)
            >>> recipe.run_build(output_dir="./output")

            >>> # Or with custom tool configuration
            >>> from rattler_build import ToolConfiguration
            >>> config = ToolConfiguration(keep_build=True, test_strategy="native")
            >>> recipe.run_build(tool_config=config, output_dir="./output")
        """
        from . import rattler_build as _rb

        # Render the recipe to get Stage1 variants
        rendered_variants = self.render(variant_config)

        # Extract the inner ToolConfiguration if provided
        tool_config_inner = tool_config._inner if tool_config else None

        # Build from the rendered variants
        _rb.build_from_rendered_variants_py(
            rendered_variants=[v._inner for v in rendered_variants],
            tool_config=tool_config_inner,
            output_dir=Path(output_dir) if output_dir else None,
            channel=channel,
            **kwargs,
        )

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class Package:
    """Package metadata at stage0."""

    def __init__(self, inner: _Stage0Package):
        self._inner = inner

    @property
    def name(self) -> Any:
        """Get the package name (may be a template string like '${{ name }}')."""
        return self._inner.name

    @property
    def version(self) -> Any:
        """Get the package version (may be a template string like '${{ version }}')."""
        return self._inner.version

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class RecipeMetadata:
    """Recipe metadata for multi-output recipes."""

    def __init__(self, inner: _Stage0RecipeMetadata):
        self._inner = inner

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class Build:
    """Build configuration at stage0."""

    def __init__(self, inner: _Stage0Build):
        self._inner = inner

    @property
    def number(self) -> Any:
        """Get the build number (may be a template)."""
        return self._inner.number

    @property
    def string(self) -> Optional[Any]:
        """Get the build string (may be a template or None for auto-generated)."""
        return self._inner.string

    @property
    def script(self) -> Any:
        """Get the build script configuration."""
        return self._inner.script

    @property
    def noarch(self) -> Optional[Any]:
        """Get the noarch type (may be a template or None)."""
        return self._inner.noarch

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class Requirements:
    """Requirements at stage0."""

    def __init__(self, inner: _Stage0Requirements):
        self._inner = inner

    @property
    def build(self) -> List[Any]:
        """Get build-time requirements (list of matchspecs or templates)."""
        return self._inner.build

    @property
    def host(self) -> List[Any]:
        """Get host-time requirements (list of matchspecs or templates)."""
        return self._inner.host

    @property
    def run(self) -> List[Any]:
        """Get run-time requirements (list of matchspecs or templates)."""
        return self._inner.run

    @property
    def run_constraints(self) -> List[Any]:
        """Get run-time constraints (list of matchspecs or templates)."""
        return self._inner.run_constraints

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class About:
    """About metadata at stage0."""

    def __init__(self, inner: _Stage0About):
        self._inner = inner

    @property
    def homepage(self) -> Optional[Any]:
        """Get the homepage URL (may be a template or None)."""
        return self._inner.homepage

    @property
    def license(self) -> Optional[Any]:
        """Get the license (may be a template or None)."""
        return self._inner.license

    @property
    def license_family(self) -> Optional[Any]:
        """Get the license family (deprecated, may be a template or None)."""
        return self._inner.license_family

    @property
    def summary(self) -> Optional[Any]:
        """Get the summary (may be a template or None)."""
        return self._inner.summary

    @property
    def description(self) -> Optional[Any]:
        """Get the description (may be a template or None)."""
        return self._inner.description

    @property
    def documentation(self) -> Optional[Any]:
        """Get the documentation URL (may be a template or None)."""
        return self._inner.documentation

    @property
    def repository(self) -> Optional[Any]:
        """Get the repository URL (may be a template or None)."""
        return self._inner.repository

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class PackageOutput:
    """A package output in a multi-output recipe."""

    def __init__(self, inner: _Stage0PackageOutput):
        self._inner = inner

    @property
    def package(self) -> Package:
        """Get the package metadata for this output."""
        return Package(self._inner.package)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class StagingOutput:
    """A staging output in a multi-output recipe."""

    def __init__(self, inner: _Stage0StagingOutput):
        self._inner = inner

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()
