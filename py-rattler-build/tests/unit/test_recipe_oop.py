from pathlib import Path
from rattler_build import Recipe, SelectorConfig
from rattler_build.rattler_build import parse_recipe_py

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
    assert build.script.strip() == "pip install . --no-deps -vv"

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
    assert not recipe.has_tests
    assert not recipe.is_noarch
    assert recipe.build.has_script
    assert not recipe.build.is_noarch


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


def test_selector_config_with_variants() -> None:
    """Test SelectorConfig with variant configuration"""
    config = SelectorConfig(target_platform="linux-64", variant={"python": "3.11", "build_number": 1})
    assert config.target_platform == "linux-64"
    assert config.variant["python"] == "3.11"
    assert config.variant["build_number"] == 1


def test_selector_config_setters() -> None:
    """Test SelectorConfig property setters"""
    config = SelectorConfig()

    initial_target = config.target_platform  # Store initial value (may have default)
    config.target_platform = "win-64"
    assert config.target_platform == "win-64"
    assert config.target_platform != initial_target

    # Test host_platform setter
    initial_host = config.host_platform
    config.host_platform = "win-64"
    assert config.host_platform == "win-64"
    assert config.host_platform != initial_host

    # Test build_platform setter
    initial_build = config.build_platform
    config.build_platform = "win-64"
    assert config.build_platform == "win-64"
    assert config.build_platform != initial_build

    # Test experimental setter
    config.experimental = True
    assert config.experimental is True
    config.experimental = False
    assert config.experimental is False

    # Test allow_undefined setter
    config.allow_undefined = True
    assert config.allow_undefined is True
    config.allow_undefined = False
    assert config.allow_undefined is False

    # Test variant setter
    initial_variant = config.variant
    new_variant = {"python": "3.9", "numpy": "1.21"}
    config.variant = new_variant
    assert config.variant == new_variant
    assert config.variant["python"] == "3.9"
    assert config.variant["numpy"] == "1.21"
    assert config.variant != initial_variant


def test_parse_recipe_with_selectors() -> None:
    """Test parsing recipe with platform selectors using existing test data"""
    linux_config = SelectorConfig(target_platform="linux-64")
    windows_config = SelectorConfig(target_platform="win-64")

    recipe_linux = parse_recipe_py(TEST_RECIPE_FILE.read_text(), linux_config.config)
    recipe_windows = parse_recipe_py(TEST_RECIPE_FILE.read_text(), windows_config.config)

    # Both should parse the same package
    assert recipe_linux["package"]["name"] == "test-package"
    assert recipe_windows["package"]["name"] == "test-package"

    # Check that we can parse both successfully - the main point is that SelectorConfig works
    # However, as @wolf noted, the "intermediate recipe" in pixi-build will be migrating to rattler-build at some point.
    # This would improve SelectorConfig to parse and validate while resolving selectors for different operating systems.
    # For example, linux: selectors could potentially be validated on Windows.
    # But for now, this is all we can do.
    assert "requirements" in recipe_linux
    assert "requirements" in recipe_windows
    assert len(recipe_linux["requirements"]["host"]) > 0
    assert len(recipe_windows["requirements"]["host"]) > 0


def test_recipe_with_variants() -> None:
    """Test recipe parsing with variant substitution using existing test data"""
    config = SelectorConfig(target_platform="linux-64", variant={"python": "3.11", "build_number": 1})

    recipe = parse_recipe_py(TEST_RECIPE_FILE.read_text(), config.config)

    assert recipe["package"]["name"] == "test-package"
    assert recipe["package"]["version"] == "1.0.0"
    assert recipe["build"]["number"] == 0
