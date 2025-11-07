"""
Stage1 - Evaluated recipe types ready for building.

This module provides Python bindings for rattler-build's stage1 types,
which represent the fully evaluated recipe with all Jinja templates resolved
and conditionals evaluated.
"""

from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    # For type checking, use Any as placeholder since we don't have stubs
    _Stage1Recipe = Any
    _Stage1Package = Any
    _Stage1Build = Any
    _Stage1Requirements = Any
    _Stage1About = Any
    _Stage1Source = Any
    _Stage1StagingCache = Any
else:
    # At runtime, import the Rust submodule
    from . import _rattler_build as _rb

    # Get the stage1 submodule
    _stage1 = _rb.stage1

    # Import classes from the stage1 submodule
    _Stage1Recipe = _stage1.Stage1Recipe
    _Stage1Package = _stage1.Stage1Package
    _Stage1Build = _stage1.Stage1Build
    _Stage1Requirements = _stage1.Stage1Requirements
    _Stage1About = _stage1.Stage1About
    _Stage1Source = _stage1.Stage1Source
    _Stage1StagingCache = _stage1.Stage1StagingCache

__all__ = [
    "Recipe",
    "Package",
    "Build",
    "Requirements",
    "About",
    "Source",
    "StagingCache",
]


class Recipe:
    """
    A fully evaluated conda recipe (stage1), ready for building.

    This represents the recipe after all Jinja templates have been evaluated
    and all conditionals resolved.

    Example:
        >>> # After parsing and evaluating a stage0 recipe
        >>> stage1_recipe = evaluate(stage0_recipe, context)
        >>> print(stage1_recipe.package.name)
        >>> print(stage1_recipe.package.version)
    """

    def __init__(self, inner: _Stage1Recipe):
        self._inner = inner

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

    @property
    def context(self) -> dict[str, Any]:
        """Get the evaluation context."""
        return self._inner.context

    @property
    def used_variant(self) -> dict[str, Any]:
        """Get the variant values used in this build."""
        return self._inner.used_variant

    @property
    def sources(self) -> list["Source"]:
        """Get all sources for this recipe."""
        return [Source(s) for s in self._inner.sources]

    @property
    def staging_caches(self) -> list["StagingCache"]:
        """Get all staging caches."""
        return [StagingCache(s) for s in self._inner.staging_caches]

    @property
    def inherits_from(self) -> dict[str, Any] | None:
        """Get inheritance information if this output inherits from a cache."""
        return self._inner.inherits_from

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class Package:
    """Package metadata at stage1 (fully evaluated)."""

    def __init__(self, inner: _Stage1Package):
        self._inner = inner

    @property
    def name(self) -> str:
        """Get the package name."""
        return self._inner.name

    @property
    def version(self) -> str:
        """Get the package version."""
        return self._inner.version

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)

    def __str__(self) -> str:
        return f"{self.name}-{self.version}"


class Build:
    """Build configuration at stage1 (fully evaluated)."""

    def __init__(self, inner: _Stage1Build):
        self._inner = inner

    @property
    def number(self) -> int:
        """Get the build number."""
        return self._inner.number

    @property
    def string(self) -> Any:
        """Get the build string."""
        return self._inner.string

    @property
    def script(self) -> Any:
        """Get the build script."""
        return self._inner.script

    @property
    def noarch(self) -> Any | None:
        """Get the noarch configuration if any."""
        return self._inner.noarch

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class Requirements:
    """Requirements at stage1 (fully evaluated)."""

    def __init__(self, inner: _Stage1Requirements):
        self._inner = inner

    @property
    def build(self) -> list[Any]:
        """Get build requirements."""
        return self._inner.build

    @property
    def host(self) -> list[Any]:
        """Get host requirements."""
        return self._inner.host

    @property
    def run(self) -> list[Any]:
        """Get run requirements."""
        return self._inner.run

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class About:
    """About metadata at stage1 (fully evaluated)."""

    def __init__(self, inner: _Stage1About):
        self._inner = inner

    @property
    def homepage(self) -> str | None:
        """Get the homepage URL."""
        return self._inner.homepage

    @property
    def repository(self) -> str | None:
        """Get the repository URL."""
        return self._inner.repository

    @property
    def documentation(self) -> str | None:
        """Get the documentation URL."""
        return self._inner.documentation

    @property
    def license(self) -> str | None:
        """Get the license string."""
        return self._inner.license

    @property
    def summary(self) -> str | None:
        """Get the summary."""
        return self._inner.summary

    @property
    def description(self) -> str | None:
        """Get the description."""
        return self._inner.description

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)


class Source:
    """Source information at stage1 (fully evaluated)."""

    def __init__(self, inner: _Stage1Source):
        self._inner = inner

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()


class StagingCache:
    """Staging cache information at stage1 (fully evaluated)."""

    def __init__(self, inner: _Stage1StagingCache):
        self._inner = inner

    @property
    def name(self) -> str:
        """Get the cache name."""
        return self._inner.name

    @property
    def build(self) -> Build:
        """Get the build configuration for this cache."""
        return Build(self._inner.build)

    @property
    def requirements(self) -> Requirements:
        """Get the requirements for this cache."""
        return Requirements(self._inner.requirements)

    def to_dict(self) -> dict[str, Any]:
        """Convert to Python dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return repr(self._inner)
