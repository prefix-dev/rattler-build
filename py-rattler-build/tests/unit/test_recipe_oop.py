import pytest
from pathlib import Path
from rattler_build import Recipe

TEST_DATA_DIR = Path(__file__).parent.parent / "data" / "recipes" / "test-package"
TEST_RECIPE_FILE = TEST_DATA_DIR / "recipe.yaml"


def test_recipe_all_sections() -> None:
    """Test accessing all recipe sections"""
    recipe = Recipe.from_file(TEST_RECIPE_FILE)

    # Package
    package = recipe.package
    assert package.name == "test-package"
    assert package.version == "1.0.0"
    assert str(package) == "test-package-1.0.0"

    # Source
    sources = recipe.source
    assert len(sources) == 1
    source = sources[0]
    assert source.source_type == "url"
    assert source.url == "https://example.com/package.tar.gz"
    assert source.sha256 == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

    # Build (including selectors)
    build = recipe.build
    assert build.number == 0
    assert not build.noarch
    assert build.script is not None

    # Requirements (including selectors)
    reqs = recipe.requirements
    # Host requirements include basic deps + conditional selectors
    assert len(reqs.host) >= 2  # At minimum python and pip
    assert len(reqs.run) >= 1  # At minimum python
    assert any("python" in req for req in reqs.run)
    assert any("pip" in req for req in reqs.host)

    # Check that selector conditions are parsed (they appear as dicts in the requirements)
    # Selectors should be preserved
    assert len(reqs.host) > 2 or len(reqs.run) > 1

    # About
    about = recipe.about
    assert about.summary == "A comprehensive test package"
    assert about.license == "MIT"
    assert about.homepage == "https://example.com/"
    assert about.repository == "https://github.com/example/test-package"

    # Empty sections
    assert len(recipe.tests) == 0
    assert len(recipe.context) == 0
    assert len(recipe.extra) == 0

    # Test convenience methods
    assert not recipe.has_tests()
    assert not recipe.is_noarch()
    assert recipe.build.has_script()
    assert not recipe.build.is_noarch()


def test_recipe_representations() -> None:
    """Test string representations"""
    recipe = Recipe.from_file(TEST_RECIPE_FILE)

    # Recipe repr
    recipe_repr = repr(recipe)
    assert "Recipe(" in recipe_repr
    assert "test-package" in recipe_repr
    assert "schema_version=1" in recipe_repr

    # Package repr and str
    package_repr = repr(recipe.package)
    assert "Package(" in package_repr
    assert "test-package" in package_repr
    assert "1.0.0" in package_repr

    # Other component reprs
    assert "Build(" in repr(recipe.build)
    assert "Requirements(" in repr(recipe.requirements)
    assert "About(" in repr(recipe.about)
    assert "Source(" in repr(recipe.source[0])


if __name__ == "__main__":
    pytest.main([__file__])
