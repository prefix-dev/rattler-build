"""
Modern recipe API tests using Stage0/Stage1/Render infrastructure.

This replaces the old test_recipe_oop.py with the new pipeline architecture.
"""

from pathlib import Path
from rattler_build.stage0 import MultiOutputRecipe, Recipe as Stage0Recipe, SingleOutputRecipe
from rattler_build.variant_config import VariantConfig
from rattler_build.render import render_recipe, RenderConfig


TEST_DATA_DIR = Path(__file__).parent.parent / "data" / "recipes" / "comprehensive-test"
TEST_RECIPE_FILE = TEST_DATA_DIR / "recipe.yaml"


def test_recipe_all_sections() -> None:
    """Test accessing all recipe sections through Stage0 and Stage1."""
    # Parse to Stage0
    stage0 = Stage0Recipe.from_file(str(TEST_RECIPE_FILE))
    assert stage0 is not None
    assert isinstance(stage0, SingleOutputRecipe)

    # Test Stage0 Package
    package_dict = stage0.package.to_dict()
    assert package_dict["name"] == "test-package"
    assert package_dict["version"] == "1.0.0"

    # Render to Stage1 for full access
    variant_config = VariantConfig()
    render_config = RenderConfig()
    rendered = render_recipe(stage0, variant_config, render_config)

    assert len(rendered) == 1
    stage1 = rendered[0].recipe()

    # Stage1 Package - fully evaluated
    assert stage1.package.name == "test-package"
    assert str(stage1.package.version) == "1.0.0"

    # Stage1 Source
    sources = stage1.sources
    assert len(sources) == 1
    source_dict = sources[0].to_dict()
    assert "url" in source_dict

    # Stage1 Build
    build = stage1.build
    assert build.number == 0
    assert build.noarch is None  # Not a noarch build

    # Stage1 Requirements - fully evaluated for target platform
    reqs = stage1.requirements
    host_reqs = reqs.host
    run_reqs = reqs.run

    # Should have python and pip at minimum
    assert len(host_reqs) >= 2
    assert len(run_reqs) >= 1

    # Stage1 About
    about = stage1.about
    assert about.summary == "A comprehensive test package"
    assert about.license == "MIT"
    assert "https://example.com" in about.homepage
    assert "https://github.com/example/test-package" in about.repository


def test_recipe_representations() -> None:
    """Test string representations of Stage0 and Stage1 objects."""
    stage0 = Stage0Recipe.from_file(str(TEST_RECIPE_FILE))

    # Stage0 repr
    recipe_repr = repr(stage0)
    assert "Stage0Recipe" in recipe_repr or "Recipe" in recipe_repr

    # Render to Stage1
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)
    stage1 = rendered[0].recipe()

    # Stage1 recipe repr
    stage1_repr = repr(stage1)
    assert "Stage1Recipe" in stage1_repr
    assert "test-package" in stage1_repr

    # Stage1 Package repr
    package_repr = repr(stage1.package)
    assert "Stage1Package" in package_repr
    assert "test-package" in package_repr
    assert "1.0.0" in package_repr

    # Other component reprs
    assert "Stage1Build" in repr(stage1.build)
    assert "Stage1Requirements" in repr(stage1.requirements)
    assert "Stage1About" in repr(stage1.about)


def test_render_config_with_variants() -> None:
    """Test RenderConfig with variant configuration."""
    render_config = RenderConfig(target_platform="linux-64")
    assert render_config.target_platform == "linux-64"

    # Set extra context (similar to variants)
    render_config.set_context("python", "3.11")
    render_config.set_context("build_number", "1")

    assert render_config.get_context("python") == "3.11"
    assert render_config.get_context("build_number") == "1"


def test_render_config_setters() -> None:
    """Test RenderConfig property setters."""
    config = RenderConfig()

    # Test target_platform - create new config to change it
    initial_target = config.target_platform
    config_win = RenderConfig(target_platform="win-64")
    assert config_win.target_platform == "win-64"
    assert config_win.target_platform != initial_target

    # Test host_platform
    config_host = RenderConfig(host_platform="win-64")
    assert config_host.host_platform == "win-64"

    # Test build_platform
    config_build = RenderConfig(build_platform="win-64")
    assert config_build.build_platform == "win-64"

    # Test experimental
    config_exp = RenderConfig(experimental=True)
    assert config_exp.experimental is True

    # Test context (variant-like values)
    config.set_context("python", "3.9")
    config.set_context("numpy", "1.21")

    all_context = config.get_all_context()
    assert "python" in all_context
    assert all_context["python"] == "3.9"
    assert "numpy" in all_context
    assert all_context["numpy"] == "1.21"


def test_parse_recipe_with_platform_selectors() -> None:
    """Test parsing recipe with platform selectors for different platforms."""
    stage0 = Stage0Recipe.from_file(str(TEST_RECIPE_FILE))
    variant_config = VariantConfig()

    # Render for Linux
    linux_config = RenderConfig(target_platform="linux-64", build_platform="linux-64", host_platform="linux-64")
    rendered_linux = render_recipe(stage0, variant_config, linux_config)
    stage1_linux = rendered_linux[0].recipe()

    # Render for Windows
    windows_config = RenderConfig(target_platform="win-64", build_platform="win-64", host_platform="win-64")
    rendered_windows = render_recipe(stage0, variant_config, windows_config)
    stage1_windows = rendered_windows[0].recipe()

    # Both should parse the same package
    assert stage1_linux.package.name == "test-package"
    assert stage1_windows.package.name == "test-package"

    # Both should have requirements, but they may differ due to selectors
    assert len(stage1_linux.requirements.host) > 0
    assert len(stage1_windows.requirements.host) > 0

    # The number of requirements might differ due to platform-specific selectors
    # For example, gcc only on unix, pywin32 only on windows
    linux_host_count = len(stage1_linux.requirements.host)
    windows_host_count = len(stage1_windows.requirements.host)

    # At minimum both should have python and pip
    assert linux_host_count >= 2
    assert windows_host_count >= 2


def test_recipe_with_variants() -> None:
    """Test recipe parsing with variant substitution."""
    yaml_content = """
package:
  name: variant-test
  version: 1.0.0

requirements:
  host:
    - python ${{ python }}.*
  run:
    - python

build:
  number: ${{ build_number }}
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)

    # Create variant config with python version
    variant_config = VariantConfig()
    variant_config.set_values("python", ["3.11"])

    # Render with context for build_number
    render_config = RenderConfig()
    render_config.set_context("build_number", "1")

    rendered = render_recipe(stage0, variant_config, render_config)
    stage1 = rendered[0].recipe()

    assert stage1.package.name == "variant-test"
    assert str(stage1.package.version) == "1.0.0"

    # Check that variant was used
    variant_dict = rendered[0].variant()
    assert "python" in variant_dict
    assert variant_dict["python"] == "3.11"

    # Build number should be evaluated from context
    assert stage1.build.number == 1


def test_stage0_to_stage1_complete_flow() -> None:
    """Test the complete flow from file to Stage1 with all features."""
    # Load from file
    stage0 = Stage0Recipe.from_file(str(TEST_RECIPE_FILE))

    assert stage0 is not None

    # Access Stage0 properties
    stage0_dict = stage0.to_dict()
    assert "package" in stage0_dict
    assert "build" in stage0_dict
    assert "requirements" in stage0_dict
    assert "about" in stage0_dict

    # Render to Stage1
    variant_config = VariantConfig()
    render_config = RenderConfig(target_platform="linux-64")
    rendered = render_recipe(stage0, variant_config, render_config)

    # Access Stage1
    variant = rendered[0]
    stage1 = variant.recipe()

    # Verify Stage1 properties are all accessible
    assert stage1.package is not None
    assert stage1.build is not None
    assert stage1.requirements is not None
    assert stage1.about is not None
    assert stage1.context is not None
    assert stage1.used_variant is not None
    assert stage1.sources is not None

    # Convert Stage1 to dict
    stage1_dict = stage1.to_dict()
    assert isinstance(stage1_dict, dict)
    assert "package" in stage1_dict


def test_multi_output_recipe_api() -> None:
    """Test API with multi-output recipes."""
    yaml_content = """
schema_version: 1

context:
  name: multi-test
  version: "2.0.0"

recipe:
  version: ${{ version }}

outputs:
  - package:
      name: ${{ name }}-lib
    requirements:
      run:
        - libfoo

  - package:
      name: ${{ name }}-dev
    requirements:
      run:
        - ${{ name }}-lib

about:
  summary: Multi-output test package
  license: MIT
"""

    # Parse Stage0
    stage0 = Stage0Recipe.from_yaml(yaml_content)
    assert isinstance(stage0, MultiOutputRecipe)
    assert stage0 is not None
    assert len(stage0.outputs) == 2

    # Render to Stage1
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    # Should have 2 outputs
    assert len(rendered) == 2

    # Check both outputs
    names = {r.recipe().package.name for r in rendered}
    assert names == {"multi-test-lib", "multi-test-dev"}

    # Both should be valid Stage1 recipes
    for variant in rendered:
        stage1 = variant.recipe()
        assert stage1.package is not None
        assert stage1.build is not None
        assert stage1.requirements is not None


def test_recipe_with_jinja_context() -> None:
    """Test recipe with Jinja2 context variables."""
    yaml_content = """
context:
  pkg_name: jinja-test
  pkg_version: "3.2.1"
  summary_text: "A test with Jinja"

package:
  name: ${{ pkg_name }}
  version: ${{ pkg_version }}

about:
  summary: ${{ summary_text }}
  license: BSD-3-Clause

build:
  number: 0
"""

    # Parse Stage0
    stage0 = Stage0Recipe.from_yaml(yaml_content)
    assert isinstance(stage0, SingleOutputRecipe)
    assert stage0 is not None

    # Check Stage0 context is preserved
    stage0_context = stage0.context
    assert "pkg_name" in stage0_context
    assert stage0_context["pkg_name"] == "jinja-test"

    # Render to Stage1
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)
    stage1 = rendered[0].recipe()

    # Jinja should be evaluated
    assert stage1.package.name == "jinja-test"
    assert str(stage1.package.version) == "3.2.1"
    assert stage1.about.summary == "A test with Jinja"

    # Context should still be accessible in Stage1
    stage1_context = stage1.context
    assert "pkg_name" in stage1_context
    assert stage1_context["pkg_name"] == "jinja-test"


def test_recipe_from_dict_api() -> None:
    """Test creating recipes from Python dictionaries."""
    recipe_dict = {
        "package": {"name": "dict-api-test", "version": "4.5.6"},
        "build": {"number": 0, "script": "echo 'Building'"},
        "requirements": {"host": ["python"], "run": ["python"]},
        "about": {"summary": "Created from dict", "license": "Apache-2.0"},
    }

    # Create Stage0 from dict
    stage0 = Stage0Recipe.from_dict(recipe_dict)

    # Render to Stage1
    variant_config = VariantConfig()
    rendered = stage0.render(variant_config)
    stage1 = rendered[0].recipe()

    # Verify all properties
    assert stage1.package.name == "dict-api-test"
    assert str(stage1.package.version) == "4.5.6"
    assert stage1.build.number == 0
    assert len(stage1.requirements.host) >= 1
    assert len(stage1.requirements.run) >= 1
    assert stage1.about.summary == "Created from dict"
    assert stage1.about.license == "Apache-2.0"
