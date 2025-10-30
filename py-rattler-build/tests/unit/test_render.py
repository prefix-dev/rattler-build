"""
Tests for the render module - converting Stage0 to Stage1 recipes with variants.
"""

from pathlib import Path
from inline_snapshot import snapshot
import pytest
from rattler_build.stage0 import Recipe
from rattler_build.variant_config import VariantConfig
from rattler_build.render import RenderConfig, RenderedVariant, render_recipe


@pytest.fixture
def test_data_dir() -> Path:
    """Fixture providing the path to the test-data directory."""
    # Go up from tests/unit/ to py-rattler-build/, then up to rattler-build/, then to test-data/
    return Path(__file__).parent.parent / "data"


def test_render_config_creation() -> None:
    """Test RenderConfig can be created with default settings."""
    config = RenderConfig()
    assert config.target_platform is not None
    assert config.build_platform is not None
    assert config.host_platform is not None
    assert not config.experimental
    assert config.recipe_path is None


def test_render_config_with_platforms() -> None:
    """Test RenderConfig with custom platforms."""
    config = RenderConfig(target_platform="linux-64", build_platform="linux-64", host_platform="linux-64")
    assert config.target_platform == "linux-64"
    assert config.build_platform == "linux-64"
    assert config.host_platform == "linux-64"


def test_render_config_set_context() -> None:
    """Test setting extra context variables."""
    config = RenderConfig()
    config.set_context("my_var", "value")
    config.set_context("my_bool", True)
    config.set_context("my_number", 42)
    config.set_context("my_list", [1, 2, 3])

    assert config.get_context("my_var") == "value"
    assert config.get_context("my_bool")
    assert isinstance(config.get_context("my_bool"), bool)
    assert config.get_context("my_number") == 42

    context = config.get_all_context()
    assert context.keys() == {"my_var", "my_bool", "my_number"}


def test_render_config_platform_setters() -> None:
    """Test platform property setters."""
    config = RenderConfig()
    config.target_platform = "osx-arm64"
    config.build_platform = "osx-64"
    config.host_platform = "linux-64"

    assert config.target_platform == "osx-arm64"
    assert config.build_platform == "osx-64"
    assert config.host_platform == "linux-64"


def test_render_config_experimental() -> None:
    """Test experimental flag."""
    config = RenderConfig(experimental=True)
    assert config.experimental

    config.experimental = False
    assert not config.experimental


def test_render_simple_recipe() -> None:
    """Test rendering a simple recipe without variants."""
    recipe_yaml = """
package:
  name: test-package
  version: 1.0.0

build:
  number: 0

requirements:
  host:
    - python >=3.8
  run:
    - python >=3.8

about:
  summary: A test package
  license: MIT
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = render_recipe(recipe, variant_config)

    assert len(rendered) == 1
    assert isinstance(rendered[0], RenderedVariant)


def test_render_recipe_with_variants() -> None:
    """Test rendering a recipe with variant configuration."""
    recipe_yaml = """
package:
  name: test-package
  version: 1.0.0

requirements:
  host:
    - python ${{ python }}.*
  run:
    - python
"""

    variant_yaml = """
python:
  - "3.9"
  - "3.10"
  - "3.11"
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig.from_yaml(variant_yaml)

    rendered = render_recipe(recipe, variant_config)

    # Should have 3 variants (one for each Python version)
    assert len(rendered) == 3

    # Check that each variant has the correct python value
    python_versions = {variant.variant().get("python") for variant in rendered}
    assert python_versions == {"3.9", "3.10", "3.11"}


def test_render_recipe_with_custom_config() -> None:
    """Test rendering with custom render configuration."""
    recipe_yaml = """
package:
  name: test-package
  version: 1.0.0
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()
    config = RenderConfig(target_platform="linux-64", experimental=True)

    rendered = render_recipe(recipe, variant_config, config)

    assert len(rendered) >= 1
    # Verify the recipe was rendered
    assert rendered[0].recipe() is not None


def test_rendered_variant_properties() -> None:
    """Test RenderedVariant properties."""
    recipe_yaml = """
package:
  name: my-package
  version: 1.2.3

build:
  number: 5
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = render_recipe(recipe, variant_config)
    variant = rendered[0]

    # Test variant() method
    variant_dict = variant.variant()
    assert isinstance(variant_dict, dict)

    # Test recipe() method
    stage1_recipe = variant.recipe()
    assert stage1_recipe is not None
    # Verify it's a Stage1 recipe by checking properties
    package = stage1_recipe.package
    assert package.name == "my-package"
    assert str(package.version) == "1.2.3"

    # Test hash_info() method
    hash_info = variant.hash_info()
    if hash_info is not None:
        assert hasattr(hash_info, "hash")
        assert hasattr(hash_info, "prefix")

    # Test pin_subpackages() method
    pin_subpackages = variant.pin_subpackages()
    assert isinstance(pin_subpackages, dict)


def test_render_multi_output_recipe() -> None:
    """Test rendering a multi-output recipe."""
    recipe_yaml = """
schema_version: 1

context:
  name: multi-pkg
  version: "1.0.0"

recipe:
  version: ${{ version }}

outputs:
  - package:
      name: ${{ name }}-lib
    build:
      noarch: generic

  - package:
      name: ${{ name }}
    build:
      noarch: generic
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = render_recipe(recipe, variant_config)

    # Should have 2 outputs
    assert len(rendered) == 2

    # Check package names
    names = {variant.recipe().package.name for variant in rendered}
    assert names == {"multi-pkg-lib", "multi-pkg"}


def test_render_with_jinja_expressions() -> None:
    """Test rendering with Jinja expressions."""
    recipe_yaml = """
package:
  name: jinja-test
  version: 1.0.0

context:
  my_value: "hello"

build:
  number: 0
  script:
    - echo "${{ my_value }}"
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = render_recipe(recipe, variant_config)

    assert len(rendered) == 1
    # Jinja should have been evaluated
    stage1_recipe = rendered[0].recipe()
    assert stage1_recipe is not None


def test_render_with_free_specs() -> None:
    """Test rendering with free specs (unversioned dependencies)."""
    recipe_yaml = """
package:
  name: test-pkg
  version: "1.0.0"

requirements:
  build:
    - python
"""

    variant_yaml = """
python:
  - "3.9"
  - "3.10"
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig.from_yaml(variant_yaml)

    rendered = render_recipe(recipe, variant_config)

    # Should create variants based on free spec "python"
    assert len(rendered) == 2


def test_render_config_repr() -> None:
    """Test RenderConfig __repr__."""
    config = RenderConfig(target_platform="linux-64", experimental=True)
    repr_str = repr(config)
    assert "RenderConfig" in repr_str
    assert "linux-64" in repr_str


def test_rendered_variant_repr() -> None:
    """Test RenderedVariant __repr__."""
    recipe_yaml = """
package:
  name: repr-test
  version: 2.0.0
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = render_recipe(recipe, variant_config)
    repr_str = repr(rendered[0])
    assert "RenderedVariant" in repr_str
    assert "repr-test" in repr_str


def test_render_with_pin_subpackage() -> None:
    """Test rendering with pin_subpackage."""
    recipe_yaml = """
schema_version: 1

context:
  name: my-pkg
  version: "0.1.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}
    build:
      noarch: generic

  - package:
      name: ${{ name }}-extra
    build:
      noarch: generic
    requirements:
      run:
        - ${{ pin_subpackage(name, exact=true) }}
"""

    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = render_recipe(recipe, variant_config)

    # Should have 2 outputs
    assert len(rendered) == 2

    # Find the -extra package
    extra_pkg = None
    for variant in rendered:
        if variant.recipe().package.name == "my-pkg-extra":
            extra_pkg = variant
            break

    assert extra_pkg is not None

    # Check pin_subpackages
    pin_subpackages = extra_pkg.pin_subpackages()
    assert "my-pkg" in pin_subpackages or "my_pkg" in pin_subpackages


def test_render_invalid_platform() -> None:
    """Test that invalid platform raises error."""
    with pytest.raises(Exception):
        RenderConfig(target_platform="invalid-platform-name")


def test_render_context_nonexistent_key() -> None:
    """Test getting non-existent context key returns None."""
    config = RenderConfig()
    assert config.get_context("nonexistent") is None


def test_render_recipe_with_staging(test_data_dir: Path) -> None:
    """Test rendering a recipe with staging."""
    recipe_path = test_data_dir / "recipes" / "with-staging.yaml"
    recipe_yaml = recipe_path.read_text()
    recipe = Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()
    rendered = render_recipe(recipe, variant_config)
    assert len(rendered) == 2
    assert isinstance(rendered[0], RenderedVariant)

    assert rendered[0].recipe().package.name == "mixed-compiled"
    assert rendered[0].recipe().package.version == "1.2.3"
    assert len(rendered[0].recipe().staging_caches) == 1
    assert rendered[0].recipe().about.to_dict() == snapshot(
        {
            "repository": "https://github.com/foobar/repo",
            "license": "Apache-2.0",
            "license_file": ["LICENSE"],
            "summary": "Compiled library package",
        }
    )


def test_render_recipe_from_yaml_string() -> None:
    """Test rendering with recipe as YAML string."""
    recipe_yaml = """
package:
  name: string-test
  version: 1.0.0
"""
    variant_yaml = """
python:
  - "3.10"
"""

    # Pass recipe and variant_config as strings
    rendered = render_recipe(recipe_yaml, variant_yaml)

    assert len(rendered) >= 1
    assert rendered[0].recipe().package.name == "string-test"


def test_render_recipe_from_path(test_data_dir: Path) -> None:
    """Test rendering with recipe as Path object."""
    recipe_path = test_data_dir / "recipes" / "with-staging.yaml"
    variant_config = VariantConfig()

    # Pass recipe as Path object
    rendered = render_recipe(recipe_path, variant_config)

    assert len(rendered) == 2
    assert rendered[0].recipe().package.name == "mixed-compiled"


def test_render_recipe_list() -> None:
    """Test rendering with a list of recipes."""
    recipe1_yaml = """
package:
  name: pkg1
  version: 1.0.0
"""
    recipe2_yaml = """
package:
  name: pkg2
  version: 2.0.0
"""

    # Parse recipes
    recipe1 = Recipe.from_yaml(recipe1_yaml)
    recipe2 = Recipe.from_yaml(recipe2_yaml)

    variant_config = VariantConfig()

    # Pass list of recipes
    rendered = render_recipe([recipe1, recipe2], variant_config)

    assert len(rendered) == 2
    names = {variant.recipe().package.name for variant in rendered}
    assert names == {"pkg1", "pkg2"}


def test_render_variant_config_from_yaml_string() -> None:
    """Test rendering with variant_config as YAML string."""
    recipe_yaml = """
package:
  name: test-pkg
  version: 1.0.0

requirements:
  host:
    - python ${{ python }}.*
"""

    variant_yaml = """
python:
  - "3.9"
  - "3.10"
"""

    # Pass variant_config as string
    rendered = render_recipe(recipe_yaml, variant_yaml)

    assert len(rendered) == 2
    python_versions = {variant.variant().get("python") for variant in rendered}
    assert python_versions == {"3.9", "3.10"}


def test_render_invalid_recipe_type() -> None:
    """Test that invalid recipe type raises TypeError."""
    with pytest.raises(TypeError, match="Unsupported recipe type"):
        render_recipe(123, VariantConfig())  # type: ignore[arg-type]


def test_render_invalid_variant_config_type() -> None:
    """Test that invalid variant_config type raises TypeError."""
    recipe_yaml = """
package:
  name: test
  version: 1.0.0
"""
    with pytest.raises(TypeError, match="Unsupported variant_config type"):
        render_recipe(recipe_yaml, 123)  # type: ignore[arg-type]
