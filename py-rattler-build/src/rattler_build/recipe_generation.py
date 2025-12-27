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
from rattler_build.stage0 import Stage0Recipe


def generate_pypi_recipe(package: str, version: str | None = None, use_mapping: bool = True) -> Stage0Recipe:
    """Generate a conda recipe from a PyPI package.

    Args:
        package: The name of the PyPI package to generate a recipe for.
        version: Specific version of the package to use. If None, uses the latest version.
        use_mapping: Whether to use conda-forge package name mappings for dependencies.
            This helps map PyPI names to their corresponding conda-forge package names.

    Returns:
        A Stage0Recipe object representing the generated recipe.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for the latest version of flit-core:

        ```python
        from rattler_build import generate_pypi_recipe

        recipe = generate_pypi_recipe("flit-core")
        recipe.as_single_output().package.name
        # 'flit-core'
        ```
    """
    yaml = generate_pypi_recipe_string_py(package, version, use_mapping)
    return Stage0Recipe.from_yaml(yaml)


def generate_cran_recipe(package: str, universe: str | None = None) -> Stage0Recipe:
    """Generate a conda recipe from a CRAN (R) package.

    Args:
        package: The name of the CRAN package to generate a recipe for.
        universe: The R universe to fetch the package from. Defaults to "cran" if not specified.
            Other options include specific R-universe repositories.

    Returns:
        A Stage0Recipe object representing the generated recipe.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for a CRAN package:

        ```python
        from rattler_build import generate_cran_recipe

        recipe = generate_cran_recipe("assertthat")
        recipe.as_single_output().package.name
        # 'r-assertthat'
        ```
    """
    yaml = generate_r_recipe_string_py(package, universe)
    return Stage0Recipe.from_yaml(yaml)


def generate_cpan_recipe(package: str, version: str | None = None) -> Stage0Recipe:
    """Generate a conda recipe from a CPAN (Perl) package.

    Args:
        package: The name of the CPAN package to generate a recipe for.
        version: Specific version of the package to use. If None, uses the latest version.

    Returns:
        A Stage0Recipe object representing the generated recipe.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for a CPAN package:

        ```python
        from rattler_build import generate_cpan_recipe

        recipe = generate_cpan_recipe("Try-Tiny")
        recipe.as_single_output().package.name
        # 'perl-try-tiny'
        ```
    """
    yaml = generate_cpan_recipe_string_py(package, version)
    return Stage0Recipe.from_yaml(yaml)


def generate_luarocks_recipe(rock: str) -> Stage0Recipe:
    """Generate a conda recipe from a LuaRocks package.

    Args:
        rock: The LuaRocks package specification. Can be in one of these formats:
            - "module" - uses the latest version
            - "module/version" - uses a specific version
            - "author/module/version" - specifies author, module and version
            - Direct rockspec URL

    Returns:
        A Stage0Recipe object representing the generated recipe.

    Raises:
        RuntimeError: If the package cannot be found or if there's a network error.

    Example:
        Generate a recipe for a LuaRocks package:

        ```python
        from rattler_build import generate_luarocks_recipe

        recipe = generate_luarocks_recipe("luafilesystem")
        recipe.as_single_output().package.name
        # 'lua-luafilesystem'
        ```
    """
    yaml = generate_luarocks_recipe_string_py(rock)
    return Stage0Recipe.from_yaml(yaml)
