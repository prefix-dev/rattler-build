"""Recipe generation for various package ecosystems.

This module provides a clean Python API for generating conda recipes
from PyPI, CRAN, CPAN, and LuaRocks packages.
"""

from typing import Optional
from .rattler_build import (
    generate_pypi_recipe_string_py,
    generate_r_recipe_string_py,
    generate_cpan_recipe_string_py,
    generate_luarocks_recipe_string_py,
)

__all__ = [
    "pypi",
    "cran",
    "cpan",
    "luarocks",
]


def pypi(package: str, version: Optional[str] = None, use_mapping: bool = True) -> str:
    """Generate a conda recipe from a PyPI package.

    Args:
        package: The name of the PyPI package to generate a recipe for.
        version: Specific version of the package to use. If None, uses the latest version.
        use_mapping: Whether to use conda-forge package name mappings for dependencies.
            This helps map PyPI names to their corresponding conda-forge package names.

    Returns:
        A string containing the generated recipe YAML.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for the latest version of numpy:

        >>> import rattler_build.recipe_generation as rg
        >>> recipe_yaml = rg.pypi("flit-core")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True

        Generate a recipe for a specific version:

        >>> recipe_yaml = rg.pypi("flit-core", version=None)  # doctest: +SKIP
    """
    return generate_pypi_recipe_string_py(package, version, use_mapping)


def cran(package: str, universe: Optional[str] = None) -> str:
    """Generate a conda recipe from a CRAN (R) package.

    Args:
        package: The name of the CRAN package to generate a recipe for.
        universe: The R universe to fetch the package from. Defaults to "cran" if not specified.
            Other options include specific R-universe repositories.

    Returns:
        A string containing the generated recipe YAML.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for a CRAN package:

        >>> import rattler_build.recipe_generation as rg
        >>> recipe_yaml = rg.cran("assertthat")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True

        Use a specific R universe:

        >>> recipe_yaml = rg.cran("assertthat", universe=None)  # doctest: +SKIP
    """
    return generate_r_recipe_string_py(package, universe)


def cpan(package: str, version: Optional[str] = None) -> str:
    """Generate a conda recipe from a CPAN (Perl) package.

    Args:
        package: The name of the CPAN package to generate a recipe for.
        version: Specific version of the package to use. If None, uses the latest version.

    Returns:
        A string containing the generated recipe YAML.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for a CPAN package:

        >>> import rattler_build.recipe_generation as rg
        >>> recipe_yaml = rg.cpan("Try-Tiny")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True

        Generate a recipe for a specific version:

        >>> recipe_yaml = rg.cpan("Try-Tiny", version=None)  # doctest: +SKIP
    """
    return generate_cpan_recipe_string_py(package, version)


def luarocks(rock: str) -> str:
    """Generate a conda recipe from a LuaRocks package.

    Args:
        rock: The LuaRocks package specification. Can be in one of these formats:
            - "module" - uses the latest version
            - "module/version" - uses a specific version
            - "author/module/version" - specifies author, module and version
            - Direct rockspec URL

    Returns:
        A string containing the generated recipe YAML.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for a LuaRocks package:

        >>> import rattler_build.recipe_generation as rg
        >>> recipe_yaml = rg.luarocks("luafilesystem")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True

        Use a specific version:

        >>> recipe_yaml = rg.luarocks("luasocket/3.1.0-1")  # doctest: +SKIP

        Use author/module/version format:

        >>> recipe_yaml = rg.luarocks("luarocks/luasocket/3.1.0-1")  # doctest: +SKIP
    """
    return generate_luarocks_recipe_string_py(rock)
