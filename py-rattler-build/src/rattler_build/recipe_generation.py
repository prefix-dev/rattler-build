"""Recipe generation for various package ecosystems.

This module provides a clean Python API for generating conda recipes
from PyPI, CRAN, CPAN, and LuaRocks packages.
"""

from rattler_build._rattler_build import (
    generate_cpan_recipe_string_py,
    generate_luarocks_recipe_string_py,
    generate_pypi_recipe_string_py,
    generate_r_recipe_string_py,
)

__all__ = [
    "generate_pypi_recipe",
    "generate_cran_recipe",
    "generate_cpan_recipe",
    "generate_luarocks_recipe",
]


def generate_pypi_recipe(package: str, version: str | None = None, use_mapping: bool = True) -> str:
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
        Generate a recipe for the latest version of flit-core:

        >>> from rattler_build import generate_pypi_recipe
        >>> recipe_yaml = generate_pypi_recipe("flit-core")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True
    """
    return generate_pypi_recipe_string_py(package, version, use_mapping)


def generate_cran_recipe(package: str, universe: str | None = None) -> str:
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

        >>> from rattler_build import generate_cran_recipe
        >>> recipe_yaml = generate_cran_recipe("assertthat")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True
    """
    return generate_r_recipe_string_py(package, universe)


def generate_cpan_recipe(package: str, version: str | None = None) -> str:
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

        >>> from rattler_build import generate_cpan_recipe
        >>> recipe_yaml = generate_cpan_recipe("Try-Tiny")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True
    """
    return generate_cpan_recipe_string_py(package, version)


def generate_luarocks_recipe(rock: str) -> str:
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

        >>> from rattler_build import generate_luarocks_recipe
        >>> recipe_yaml = generate_luarocks_recipe("luafilesystem")  # doctest: +SKIP
        >>> "package:" in recipe_yaml  # doctest: +SKIP
        True
    """
    return generate_luarocks_recipe_string_py(rock)
