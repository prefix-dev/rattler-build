"""
Tests for the render module - converting Stage0 to Stage1 recipes with variants.
"""

from pathlib import Path

import pytest
from inline_snapshot import snapshot

from rattler_build import (
    PlatformConfig,
    PlatformParseError,
    RenderConfig,
    RenderedVariant,
    Stage0Recipe,
    VariantConfig,
)


@pytest.fixture
def test_data_dir() -> Path:
    """Fixture providing the path to the test-data directory."""
    # Go up from tests/unit/ to py-rattler-build/, then up to rattler-build/, then to test-data/
    return Path(__file__).parent.parent / "data"


def test_render_config_with_platforms() -> None:
    """Test RenderConfig with custom platforms."""
    platform_config = PlatformConfig("linux-64")
    config = RenderConfig(platform=platform_config)
    assert config.target_platform == "linux-64"
    assert config.build_platform == "linux-64"
    assert config.host_platform == "linux-64"


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

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig.from_yaml(variant_yaml)

    rendered = recipe.render(variant_config)

    # Should have 3 variants (one for each Python version)
    assert len(rendered) == 3

    # Check that each variant has the correct python value
    python_versions = {variant.variant().get("python") for variant in rendered}
    assert python_versions == {"3.9", "3.10", "3.11"}


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

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = recipe.render(variant_config)

    # Should have 2 outputs
    assert len(rendered) == 2

    # Check package names
    names = {variant.recipe().package.name for variant in rendered}
    assert names == {"multi-pkg-lib", "multi-pkg"}


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

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    rendered = recipe.render(variant_config)

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
    """Test that invalid platform raises PlatformParseError."""
    with pytest.raises(PlatformParseError, match="'invalid-platform' is not a known platform."):
        platform_config = PlatformConfig("invalid-platform")
        RenderConfig(platform=platform_config)


def test_render_context_nonexistent_key() -> None:
    """Test getting non-existent context key returns None."""
    config = RenderConfig()
    assert config.get_context("nonexistent") is None


def test_render_recipe_with_staging(test_data_dir: Path) -> None:
    """Test rendering a recipe with staging."""
    recipe_path = test_data_dir / "recipes" / "with-staging.yaml"
    recipe_yaml = recipe_path.read_text()
    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()
    render_config = RenderConfig(platform=PlatformConfig(experimental=True))
    rendered = recipe.render(variant_config, render_config)
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

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig.from_yaml(variant_yaml)

    rendered = recipe.render(variant_config)

    assert len(rendered) >= 1
    assert rendered[0].recipe().package.name == "string-test"


def test_render_recipe_from_path(test_data_dir: Path) -> None:
    """Test rendering with recipe as Path object."""
    recipe_path = test_data_dir / "recipes" / "with-staging.yaml"
    recipe = Stage0Recipe.from_file(recipe_path)
    variant_config = VariantConfig()
    render_config = RenderConfig(platform=PlatformConfig(experimental=True))

    rendered = recipe.render(variant_config, render_config)

    assert len(rendered) == 2
    assert rendered[0].recipe().package.name == "mixed-compiled"


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
    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig.from_yaml(variant_yaml)

    rendered = recipe.render(variant_config)

    assert len(rendered) == 2
    python_versions = {variant.variant().get("python") for variant in rendered}
    assert python_versions == {"3.9", "3.10"}


def test_run_build_exclude_newer_datetime_conversion(tmp_path: Path) -> None:
    """Test that Python datetime converts correctly to Rust chrono::DateTime<Utc>."""
    from datetime import datetime, timezone

    recipe_yaml = """
package:
  name: datetime-test
  version: 1.0.0

build:
  number: 0
  noarch: generic
"""

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()
    rendered = recipe.render(variant_config)

    # Create a timezone-aware datetime (required for chrono::DateTime<Utc>)
    exclude_newer = datetime(2024, 1, 1, 12, 0, 0, tzinfo=timezone.utc)

    result = rendered[0].run_build(output_dir=tmp_path, exclude_newer=exclude_newer)
    assert result.name == "datetime-test"
