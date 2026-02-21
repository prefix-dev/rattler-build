"""
Stage0 - Parsed recipe types before evaluation.

This module provides Python bindings for rattler-build's stage0 types,
which represent the parsed YAML recipe before Jinja template evaluation
and conditional resolution.
"""

from __future__ import annotations

import json
import tempfile
from abc import ABC, abstractmethod
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any

from rattler_build._rattler_build import render as _render
from rattler_build._rattler_build import stage0 as _stage0
from rattler_build.render import RenderConfig, RenderedVariant
from rattler_build.tool_config import ToolConfiguration
from rattler_build.variant_config import VariantConfig

if TYPE_CHECKING:
    from rattler_build.build_result import BuildResult
    from rattler_build.progress import ProgressCallback


class Stage0Recipe(ABC):
    """
    A parsed conda recipe (stage0).

    This is an abstract base class. Use `from_yaml()`, `from_file()`, or `from_dict()`
    to create concrete instances (`SingleOutputRecipe` or `MultiOutputRecipe`).

    Example:
        ```python
        recipe = Stage0Recipe.from_yaml(yaml_string)
        if isinstance(recipe, SingleOutputRecipe):
            print(recipe.package.name)
        ```
    """

    # Attributes set by concrete subclasses
    _inner: _stage0.SingleOutputRecipe | _stage0.MultiOutputRecipe
    _wrapper: _stage0.Stage0Recipe
    _recipe_path: Path

    @property
    def recipe_path(self) -> Path:
        """Get the path to the recipe file on disk.

        This is always set:
        - ``from_file()`` uses the provided file path.
        - ``from_yaml()`` / ``from_dict()`` write the recipe to ``recipe_dir``
          (or a temporary directory when ``recipe_dir`` is not given).
        """
        return self._recipe_path

    @classmethod
    def _write_recipe(cls, content: str, recipe_dir: Path | str | None) -> Path:
        """Write recipe content to a directory, returning the recipe file path.

        If *recipe_dir* is ``None`` a temporary directory is created automatically.
        """
        if recipe_dir is None:
            recipe_dir = Path(tempfile.mkdtemp(prefix="rattler_build_"))
        else:
            recipe_dir = Path(recipe_dir)

        recipe_dir.mkdir(parents=True, exist_ok=True)
        recipe_path = recipe_dir / "recipe.yaml"
        recipe_path.write_text(content, encoding="utf-8")
        return recipe_path

    @classmethod
    def from_yaml(cls, yaml: str, *, recipe_dir: Path | str | None = None) -> Stage0Recipe:
        """Parse a recipe from a YAML string.

        Args:
            yaml: The YAML recipe content.
            recipe_dir: Directory to write the recipe file into.  When
                ``None`` (the default) a temporary directory is created.

        Returns:
            SingleOutputRecipe or MultiOutputRecipe depending on the recipe type.
        """
        recipe_path = cls._write_recipe(yaml, recipe_dir)

        wrapper = _stage0.Stage0Recipe.from_yaml(yaml)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper, recipe_path)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper, recipe_path)

    @classmethod
    def from_file(cls, path: str | Path) -> Stage0Recipe:
        """Parse a recipe from a YAML file.

        The file path is used as the recipe path directly — no copy is made.

        Returns:
            SingleOutputRecipe or MultiOutputRecipe depending on the recipe type.
        """
        path = Path(path).resolve()
        with open(path, encoding="utf-8") as f:
            yaml = f.read()

        wrapper = _stage0.Stage0Recipe.from_yaml(yaml)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper, path)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper, path)

    @classmethod
    def from_dict(cls, recipe_dict: dict[str, Any], *, recipe_dir: Path | str | None = None) -> Stage0Recipe:
        """Create a recipe from a Python dictionary.

        This method validates the dictionary structure and provides detailed error
        messages if the structure is invalid or types don't match.

        Args:
            recipe_dict: Dictionary containing recipe data (must match recipe schema).
            recipe_dir: Directory to write the recipe file into.  When
                ``None`` (the default) a temporary directory is created.

        Returns:
            SingleOutputRecipe or MultiOutputRecipe depending on the recipe type.

        Raises:
            PyRecipeParseError: If the dictionary structure is invalid or types don't match.

        Example:
            ```python
            recipe_dict = {
                "package": {
                    "name": "my-package",
                    "version": "1.0.0"
                },
                "build": {
                    "number": 0
                }
            }
            recipe = Stage0Recipe.from_dict(recipe_dict)
            ```
        """
        recipe_path = cls._write_recipe(json.dumps(recipe_dict, indent=2), recipe_dir)

        wrapper = _stage0.Stage0Recipe.from_dict(recipe_dict)
        if wrapper.is_single_output():
            single_inner = wrapper.as_single_output()
            return SingleOutputRecipe(single_inner, wrapper, recipe_path)
        else:
            multi_inner = wrapper.as_multi_output()
            return MultiOutputRecipe(multi_inner, wrapper, recipe_path)

    def as_single_output(self) -> SingleOutputRecipe:
        """
        Get as a single output recipe.

        Raises:
            TypeError: If this is not a single-output recipe.
        """
        if not isinstance(self, SingleOutputRecipe):
            raise TypeError(f"Recipe is not a single-output recipe, it's a {type(self).__name__}")
        return self

    def as_multi_output(self) -> MultiOutputRecipe:
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
    def build(self) -> Build:
        """Get the build configuration."""
        ...

    @property
    @abstractmethod
    def about(self) -> About:
        """Get the about metadata."""
        ...

    def render(
        self, variant_config: VariantConfig | None = None, render_config: RenderConfig | None = None
    ) -> list[RenderedVariant]:
        """
        Render this recipe with variant configuration.

        This method takes this Stage0 recipe and evaluates all Jinja templates
        with different variant combinations to produce ready-to-build Stage1 recipes.

        The recipe's :attr:`recipe_path` is automatically injected into the
        render configuration so that Jinja functions like ``include()`` and
        ``file_name()`` can resolve relative paths.

        Args:
            variant_config: Optional VariantConfig to use. If None, creates an empty config.
            render_config: Optional RenderConfig to use. If None, uses default config.

        Returns:
            List of RenderedVariant objects (one for each variant combination)

        Example:
            ```python
            recipe = Stage0Recipe.from_yaml(yaml_string)
            variants = recipe.render(variant_config)
            for variant in variants:
                print(variant.recipe.package.name)
            ```
        """
        # Create empty variant config if not provided
        if variant_config is None:
            variant_config = VariantConfig()

        # Build a RenderConfig with the recipe_path injected
        render_config_inner = RenderConfig._with_recipe_path(render_config, self._recipe_path)

        # Unwrap variant_config to get inner Rust object
        variant_config_inner = variant_config._inner

        # Render the recipe using the wrapper
        rendered = _render.render_recipe(self._wrapper, variant_config_inner, render_config_inner)

        return [RenderedVariant(r, self._recipe_path) for r in rendered]

    def run_build(
        self,
        variant_config: VariantConfig | None = None,
        tool_config: ToolConfiguration | None = None,
        output_dir: str | Path | None = None,
        channels: list[str] | None = None,
        progress_callback: ProgressCallback | None = None,
        no_build_id: bool = False,
        package_format: str | None = None,
        no_include_recipe: bool = False,
        debug: bool = False,
        exclude_newer: datetime | None = None,
    ) -> list[BuildResult]:
        """Build this recipe.

        This method renders the recipe with variants and then builds the rendered
        outputs.  The :attr:`recipe_path` is used automatically for directory
        setup and recipe inclusion in the package.

        Args:
            variant_config: Optional VariantConfig to use for building variants.
            tool_config: ToolConfiguration to use for the build. If None, uses defaults.
            output_dir: Directory to store the built packages.
                Defaults to ``<recipe_dir>/output``.
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
            recipe = Stage0Recipe.from_yaml(yaml_string)
            # Build with default output dir (<recipe_dir>/output)
            results = recipe.run_build()
            for result in results:
                print(f"Built {result.name} {result.version}")
                print(f"Package at: {result.packages[0]}")

            # Or with custom tool configuration
            from rattler_build import ToolConfiguration
            config = ToolConfiguration(keep_build=True, test_strategy="native")
            results = recipe.run_build(tool_config=config, output_dir="./output")
            ```
        """

        # Render the recipe to get Stage1 variants
        rendered_variants = self.render(variant_config)

        # Build each rendered variant using its run_build method
        results: list[BuildResult] = []
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


class SingleOutputRecipe(Stage0Recipe):
    """A single-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _stage0.SingleOutputRecipe, wrapper: _stage0.Stage0Recipe, recipe_path: Path):
        self._inner = inner
        self._wrapper = wrapper
        self._recipe_path = recipe_path

    @property
    def schema_version(self) -> int:
        """Get the schema version."""
        return self._inner.schema_version

    @property
    def context(self) -> dict[str, Any]:
        """Get the context variables as a dictionary."""
        return self._inner.context

    @property
    def package(self) -> Package:
        """Get the package metadata."""
        return Package(self._inner.package)

    @property
    def build(self) -> Build:
        """Get the build configuration."""
        return Build(self._inner.build)

    @property
    def requirements(self) -> Requirements:
        """Get the requirements."""
        return Requirements(self._inner.requirements)

    @property
    def about(self) -> About:
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


class MultiOutputRecipe(Stage0Recipe):
    """A multi-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _stage0.MultiOutputRecipe, wrapper: _stage0.Stage0Recipe, recipe_path: Path):
        self._inner = inner
        self._wrapper = wrapper
        self._recipe_path = recipe_path

    @property
    def schema_version(self) -> int:
        """Get the schema version."""
        return self._inner.schema_version

    @property
    def context(self) -> dict[str, Any]:
        """Get the context variables as a dictionary."""
        return self._inner.context

    @property
    def recipe(self) -> RecipeMetadata:
        """Get the top-level recipe metadata."""
        return RecipeMetadata(self._inner.recipe)

    @property
    def build(self) -> Build:
        """Get the top-level build configuration."""
        return Build(self._inner.build)

    @property
    def about(self) -> About:
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
