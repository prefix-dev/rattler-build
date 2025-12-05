from rattler_build import (
    generate_cpan_recipe,
    generate_cran_recipe,
    generate_luarocks_recipe,
    generate_pypi_recipe,
)


def test_generate_pypi_recipe_smoke() -> None:
    recipe = generate_pypi_recipe("flit-core").as_single_output()
    assert recipe.package.name == "flit-core"
    assert recipe.about.summary is not None


def test_generate_cran_recipe_smoke() -> None:
    recipe = generate_cran_recipe("assertthat").as_single_output()
    assert recipe.package.name == "r-assertthat"
    assert recipe.about.license is not None


def test_generate_cpan_recipe_smoke() -> None:
    recipe = generate_cpan_recipe("Try-Tiny").as_single_output()
    assert recipe.package.name == "perl-try-tiny"
    assert recipe.about.license is not None


def test_generate_luarocks_recipe_smoke() -> None:
    recipe = generate_luarocks_recipe("luafilesystem").as_single_output()
    assert recipe.package.name == "lua-luafilesystem"
    assert recipe.about.license is not None


def test_generate_pypi_recipe_with_use_mapping_false() -> None:
    """Test PyPI recipe generation with use_mapping=False."""
    recipe = generate_pypi_recipe("flit-core", use_mapping=False).as_single_output()
    assert recipe.package.name == "flit-core"
    assert recipe.about.summary is not None


def test_generate_cran_recipe_with_explicit_universe() -> None:
    """Test CRAN recipe generation with explicit universe parameter."""
    recipe = generate_cran_recipe("assertthat", universe="cran").as_single_output()
    assert recipe.package.name == "r-assertthat"
    assert recipe.about.license is not None
