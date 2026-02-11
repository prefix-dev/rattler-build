from rattler_build import (
    generate_cran_recipe,
    generate_pypi_recipe,
)


def test_generate_pypi_recipe_smoke() -> None:
    recipe = generate_pypi_recipe("boltons").as_single_output()
    assert recipe.package.name == "boltons"
    assert recipe.about.summary is not None


def test_generate_cran_recipe_smoke() -> None:
    recipe = generate_cran_recipe("assertthat").as_single_output()
    assert recipe.package.name == "r-assertthat"
    assert recipe.about.license is not None


def test_generate_cran_recipe_with_explicit_universe() -> None:
    """Test CRAN recipe generation with explicit universe parameter."""
    recipe = generate_cran_recipe("assertthat", universe="cran").as_single_output()
    assert recipe.package.name == "r-assertthat"
    assert recipe.about.license is not None
