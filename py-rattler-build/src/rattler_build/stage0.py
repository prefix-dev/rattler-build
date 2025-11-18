"""
Stage0 - Parsed recipe types before evaluation.

This module provides Python bindings for rattler-build's stage0 types,
which represent the parsed YAML recipe before Jinja template evaluation
and conditional resolution.
"""

from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any

from rattler_build._rattler_build import render as _render
from rattler_build._rattler_build import stage0 as _stage0
from rattler_build.render import RenderConfig, RenderedVariant
from rattler_build.tool_config import ToolConfiguration
from rattler_build.variant_config import VariantConfig

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


class Recipe(ABC):
    """
    A parsed conda recipe (stage0).

    This is an abstract base class. Use from_yaml(), from_file(), or from_dict()
    to create concrete instances (SingleOutputRecipe or MultiOutputRecipe).

    Example:
        >>> recipe = Recipe.from_yaml(yaml_string)
        >>> if isinstance(recipe, SingleOutputRecipe):
        ...     print(recipe.package.name)
    """

    # Attributes set by concrete subclasses
    _inner: _stage0.SingleOutputRecipe | _stage0.MultiOutputRecipe
    _wrapper: _stage0.Stage0Recipe

    @classmethod
    def from_yaml(cls, yaml: str) -> "Recipe":
        """
        Parse a recipe from YAML string.

        Returns the appropriate type: SingleOutputRecipe or MultiOutputRecipe.
        """
        wrapper = _stage0.Stage0Recipe.from_yaml(yaml)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper)

    @classmethod
    def from_file(cls, path: str | Path) -> "Recipe":
        """
        Parse a recipe from a YAML file.

        Returns the appropriate type: SingleOutputRecipe or MultiOutputRecipe.
        """
        with open(path, encoding="utf-8") as f:
            return cls.from_yaml(f.read())

    @classmethod
    def from_dict(cls, recipe_dict: dict[str, Any]) -> "Recipe":
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
        wrapper = _stage0.Stage0Recipe.from_dict(recipe_dict)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper)

    def as_single_output(self) -> "SingleOutputRecipe":
        """
        Get as a single output recipe.

        Raises:
            TypeError: If this is not a single-output recipe.
        """
        if not isinstance(self, SingleOutputRecipe):
            raise TypeError(f"Recipe is not a single-output recipe, it's a {type(self).__name__}")
        return self

    def as_multi_output(self) -> "MultiOutputRecipe":
        """
        Get as a multi output recipe.

        Raises:
            TypeError: If this is not a multi-output recipe.
        """
        if not isinstance(self, MultiOutputRecipe):
            raise TypeError(f"Recipe is not a multi-output recipe, it's a {type(self).__name__}")
        return self

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)

    @property
    @abstractmethod
    def schema_version(self) -> int:
        """Get the schema version."""
        ...

    @property
    @abstractmethod
    def context(self) -> dict[str, Any]:
        """Get the context variables as a dictionary."""
        ...

    @property
    @abstractmethod
    def build(self) -> "Build":
        """Get the build configuration."""
        ...

    @property
    @abstractmethod
    def about(self) -> "About":
        """Get the about metadata."""
        ...

    def render(
        self, variant_config: VariantConfig | None = None, render_config: RenderConfig | None = None
    ) -> list[RenderedVariant]:
        """
        Render this recipe with variant configuration.

        This method takes this Stage0 recipe and evaluates all Jinja templates
        with different variant combinations to produce ready-to-build Stage1 recipes.

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
        # Create empty variant config if not provided
        if variant_config is None:
            variant_config = VariantConfig()

        # Handle render_config parameter
        render_config_inner = None if render_config is None else render_config._config

        # Unwrap variant_config to get inner Rust object
        variant_config_inner = variant_config._inner

        # Render the recipe using the wrapper
        rendered = _render.render_recipe(self._wrapper, variant_config_inner, render_config_inner)

        return [RenderedVariant(r) for r in rendered]

    def run_build(
        self,
        variant_config: VariantConfig | None = None,
        tool_config: ToolConfiguration | None = None,
        output_dir: str | Path | None = None,
        channel: list[str] | None = None,
        progress_callback: Any | None = None,
        recipe_path: str | Path | None = None,
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
            progress_callback: Optional progress callback for build events (e.g., RichProgressCallback or SimpleProgressCallback).
            recipe_path: Path to the recipe file (for copying license files, etc.). Defaults to None.
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
        # Render the recipe to get Stage1 variants
        rendered_variants = self.render(variant_config)

        # Build each rendered variant using its run_build method
        for variant in rendered_variants:
            variant.run_build(
                tool_config=tool_config,
                output_dir=output_dir,
                channel=channel,
                progress_callback=progress_callback,
                recipe_path=recipe_path,
                **kwargs,
            )


class SingleOutputRecipe(Recipe):
    """A single-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _stage0.SingleOutputRecipe, wrapper: _stage0.Stage0Recipe):
        self._inner = inner
        self._wrapper = wrapper

    @property
    def schema_version(self) -> int:
        """Get the schema version."""
        return self._inner.schema_version

    @property
    def context(self) -> dict[str, Any]:
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


class Package:
    """Package metadata at stage0."""

    def __init__(self, inner: _stage0.Stage0Package):
        self._inner = inner

    @property
    def name(self) -> Any:
        """Get the package name (may be a template string like '${{ name }}')."""
        return self._inner.name

    @property
    def version(self) -> Any:
        """Get the package version (may be a template string like '${{ version }}')."""
        return self._inner.version

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class PackageOutput:
    """A package output in a multi-output recipe."""

    def __init__(self, inner: _stage0.Stage0PackageOutput):
        self._inner = inner

    @property
    def package(self) -> Package:
        """Get the package metadata for this output."""
        return Package(self._inner.package)

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class StagingOutput:
    """A staging output in a multi-output recipe."""

    def __init__(self, inner: _stage0.Stage0StagingOutput):
        self._inner = inner

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class MultiOutputRecipe(Recipe):
    """A multi-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _stage0.MultiOutputRecipe, wrapper: _stage0.Stage0Recipe):
        self._inner = inner
        self._wrapper = wrapper

    @property
    def schema_version(self) -> int:
        """Get the schema version."""
        return self._inner.schema_version

    @property
    def context(self) -> dict[str, Any]:
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
    def outputs(self) -> list[PackageOutput | StagingOutput]:
        """Get all outputs (package and staging)."""
        result: list[PackageOutput | StagingOutput] = []
        for output in self._inner.outputs:
            if isinstance(output, _stage0.Stage0PackageOutput):
                result.append(PackageOutput(output))
            elif isinstance(output, _stage0.Stage0StagingOutput):
                result.append(StagingOutput(output))
        return result


class RecipeMetadata:
    """Recipe metadata for multi-output recipes."""

    def __init__(self, inner: _stage0.Stage0RecipeMetadata):
        self._inner = inner

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class Build:
    """Build configuration at stage0."""

    def __init__(self, inner: _stage0.Stage0Build):
        self._inner = inner

    @property
    def number(self) -> Any:
        """Get the build number (may be a template)."""
        return self._inner.number

    @property
    def string(self) -> Any | None:
        """Get the build string (may be a template or None for auto-generated)."""
        return self._inner.string

    @property
    def script(self) -> Any:
        """Get the build script configuration."""
        return self._inner.script

    @property
    def noarch(self) -> Any | None:
        """Get the noarch type (may be a template or None)."""
        return self._inner.noarch

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class Requirements:
    """Requirements at stage0."""

    def __init__(self, inner: _stage0.Stage0Requirements):
        self._inner = inner

    @property
    def build(self) -> list[Any]:
        """Get build-time requirements (list of matchspecs or templates)."""
        return self._inner.build

    @property
    def host(self) -> list[Any]:
        """Get host-time requirements (list of matchspecs or templates)."""
        return self._inner.host

    @property
    def run(self) -> list[Any]:
        """Get run-time requirements (list of matchspecs or templates)."""
        return self._inner.run

    @property
    def run_constraints(self) -> list[Any]:
        """Get run-time constraints (list of matchspecs or templates)."""
        return self._inner.run_constraints

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class About:
    """About metadata at stage0."""

    def __init__(self, inner: _stage0.Stage0About):
        self._inner = inner

    @property
    def homepage(self) -> Any | None:
        """Get the homepage URL (may be a template or None)."""
        return self._inner.homepage

    @property
    def license(self) -> Any | None:
        """Get the license (may be a template or None)."""
        return self._inner.license

    @property
    def license_family(self) -> Any | None:
        """Get the license family (deprecated, may be a template or None)."""
        return self._inner.license_family

    @property
    def summary(self) -> Any | None:
        """Get the summary (may be a template or None)."""
        return self._inner.summary

    @property
    def description(self) -> Any | None:
        """Get the description (may be a template or None)."""
        return self._inner.description

    @property
    def documentation(self) -> Any | None:
        """Get the documentation URL (may be a template or None)."""
        return self._inner.documentation

    @property
    def repository(self) -> Any | None:
        """Get the repository URL (may be a template or None)."""
        return self._inner.repository

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()
