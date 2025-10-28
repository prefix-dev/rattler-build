"""
Stage0 - Parsed recipe types before evaluation.

This module provides Python bindings for rattler-build's stage0 types,
which represent the parsed YAML recipe before Jinja template evaluation
and conditional resolution.
"""

from pathlib import Path
from typing import Any, Dict, List, Optional, Union, TYPE_CHECKING

if TYPE_CHECKING:
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
    def from_yaml(cls, yaml: str) -> "Recipe":
        """Parse a recipe from YAML string."""
        return cls(_Stage0Recipe.from_yaml(yaml))

    @classmethod
    def from_file(cls, path: Union[str, Path]) -> "Recipe":
        """Parse a recipe from a YAML file."""
        with open(path, "r", encoding="utf-8") as f:
            return cls.from_yaml(f.read())

    @classmethod
    def from_dict(cls, recipe_dict: Dict[str, Any]) -> "Recipe":
        """
        Create a recipe from a Python dictionary.

        This method validates the dictionary structure and provides detailed error
        messages if the structure is invalid or types don't match.

        Args:
            recipe_dict: Dictionary containing recipe data (must match recipe schema)

        Returns:
            A new Recipe instance

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
        return cls(_Stage0Recipe.from_dict(recipe_dict))

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

    def __init__(self, inner: _SingleOutputRecipe):
        self._inner = inner

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

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class MultiOutputRecipe:
    """A multi-output recipe at stage0 (parsed, not yet evaluated)."""

    def __init__(self, inner: _MultiOutputRecipe):
        self._inner = inner

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

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class Package:
    """Package metadata at stage0."""

    def __init__(self, inner: _Stage0Package):
        self._inner = inner

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

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class Requirements:
    """Requirements at stage0."""

    def __init__(self, inner: _Stage0Requirements):
        self._inner = inner

    def to_dict(self) -> Dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class About:
    """About metadata at stage0."""

    def __init__(self, inner: _Stage0About):
        self._inner = inner

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
