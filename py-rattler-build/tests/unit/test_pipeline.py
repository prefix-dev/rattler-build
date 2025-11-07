"""
Comprehensive tests for the full rattler-build pipeline.

Tests the complete flow: Stage0 Recipe -> Render -> Stage1 Recipe
"""

import pytest
from rattler_build.stage0 import MultiOutputRecipe, Recipe as Stage0Recipe, SingleOutputRecipe
from rattler_build.variant_config import VariantConfig
from rattler_build.render import render_recipe, RenderConfig


def test_pipeline_from_yaml_to_stage1() -> None:
    """Test complete pipeline from YAML to Stage1 recipe."""
    yaml_content = """
package:
  name: my-package
  version: 1.0.0

build:
  number: 0

requirements:
  host:
    - python
  run:
    - python
"""

    # Step 1: Parse to Stage0
    stage0 = Stage0Recipe.from_yaml(yaml_content)
    assert isinstance(stage0, SingleOutputRecipe)

    # Step 2: Create variant config
    variant_config = VariantConfig()

    # Step 3: Render to get Stage1
    rendered = render_recipe(stage0, variant_config)
    assert len(rendered) == 1

    # Step 4: Access Stage1 recipe
    stage1 = rendered[0].recipe()
    # Note: The Python wrapper class name might differ from the import
    assert stage1 is not None
    assert stage1.package.name == "my-package"
    assert str(stage1.package.version) == "1.0.0"


def test_pipeline_from_dict_to_stage1() -> None:
    """Test complete pipeline from Python dict to Stage1 recipe."""
    recipe_dict = {
        "package": {"name": "dict-package", "version": "2.0.0"},
        "build": {"number": 0},
        "requirements": {"run": ["numpy"]},
    }

    # Step 1: Create Stage0 from dict
    stage0 = Stage0Recipe.from_dict(recipe_dict)
    assert isinstance(stage0, SingleOutputRecipe)

    # Step 2: Render
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    # Step 3: Verify Stage1
    stage1 = rendered[0].recipe()
    assert stage1.package.name == "dict-package"
    assert stage1.package.version == "2.0.0"


def test_pipeline_with_variants() -> None:
    """Test pipeline with variant combinations."""
    yaml_content = """
package:
  name: variant-package
  version: 1.0.0

requirements:
  host:
    - python
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)

    # Create variant config with multiple python versions
    variant_config = VariantConfig({"python": ["3.9", "3.10", "3.11"]})

    # Render with variants
    rendered = render_recipe(stage0, variant_config)

    # Should have 3 variants
    assert len(rendered) == 3

    # Each variant should have different python version in used_variant
    for i, variant in enumerate(rendered):
        stage1 = variant.recipe()
        assert stage1.package.name == "variant-package"
        # Check that the variant was used
        assert "python" in variant.variant()


def test_pipeline_multi_output() -> None:
    """Test pipeline with multi-output recipe."""
    yaml_content = """
recipe:
  name: multi-package
  version: 1.0.0

outputs:
  - package:
      name: multi-lib
    requirements:
      run:
        - libfoo

  - package:
      name: multi-dev
    requirements:
      run:
        - multi-lib
"""

    # Parse Stage0
    stage0 = Stage0Recipe.from_yaml(yaml_content)
    assert isinstance(stage0, MultiOutputRecipe)

    # Render
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    # Should have 2 outputs
    assert len(rendered) == 2

    # Check output names
    names = {r.recipe().package.name for r in rendered}
    assert names == {"multi-lib", "multi-dev"}


def test_pipeline_with_render_config() -> None:
    """Test pipeline with custom RenderConfig."""
    yaml_content = """
package:
  name: platform-package
  version: 1.0.0
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()

    # Create custom render config for linux
    render_config = RenderConfig(target_platform="linux-64")

    # Render with custom config
    rendered = render_recipe(stage0, variant_config, render_config)

    assert len(rendered) == 1
    stage1 = rendered[0].recipe()
    assert stage1.package.name == "platform-package"


def test_pipeline_hash_info() -> None:
    """Test that hash_info is available after rendering."""
    yaml_content = """
package:
  name: hash-package
  version: 1.0.0
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    # Hash info should be available
    hash_info = rendered[0].hash_info()
    assert hash_info is not None
    assert isinstance(hash_info.hash, str)
    assert len(hash_info.hash) == 7


def test_pipeline_pin_subpackages() -> None:
    """Test pin_subpackage information in pipeline."""
    yaml_content = """
recipe:
  name: pin-test

outputs:
  - package:
      name: pin-lib
      version: 1.0.0

  - package:
      name: pin-app
      version: 1.0.0
    requirements:
      host:
        - ${{ pin_subpackage('pin-lib', exact=True) }}
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    # Find the pin-app output
    app_variant = None
    for variant in rendered:
        if variant.recipe().package.name == "pin-app":
            app_variant = variant
            break

    assert app_variant is not None

    # Check pin_subpackages
    pins = app_variant.pin_subpackages()
    assert "pin-lib" in pins
    assert pins["pin-lib"].exact is True


def test_pipeline_stage0_to_dict() -> None:
    """Test that Stage0 can be converted to dict."""
    yaml_content = """
package:
  name: roundtrip-package
  version: 1.0.0

build:
  number: 5
"""

    # Parse to Stage0
    stage0_original = Stage0Recipe.from_yaml(yaml_content)

    # Convert to dict
    recipe_dict = stage0_original.to_dict()
    assert isinstance(recipe_dict, dict)
    assert recipe_dict["package"]["name"] == "roundtrip-package"

    # Note: Round-tripping from to_dict() output may not work directly
    # because to_dict() includes all defaults with their serialized form
    # which may not always be valid for from_dict()
    # For practical use, users should construct minimal dicts manually

    # Just verify we can render the original
    variant_config = VariantConfig()
    rendered_original = render_recipe(stage0_original, variant_config)

    assert len(rendered_original) == 1
    assert rendered_original[0].recipe().package.name == "roundtrip-package"


def test_pipeline_stage1_properties() -> None:
    """Test accessing all Stage1 recipe properties."""
    yaml_content = """
context:
  version: 1.0.0

package:
  name: props-package
  version: ${{ version }}

build:
  number: 0

requirements:
  build:
    - cmake
  host:
    - python
  run:
    - python

about:
  summary: A test package
  license: MIT
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    stage1 = rendered[0].recipe()

    # Test all properties
    assert stage1.package.name == "props-package"
    assert stage1.package.version == "1.0.0"

    assert stage1.build is not None

    assert stage1.requirements is not None

    assert stage1.about is not None

    assert isinstance(stage1.context, dict)

    assert isinstance(stage1.used_variant, dict)


def test_pipeline_used_variant() -> None:
    """Test that used_variant contains the variant values."""
    yaml_content = """
package:
  name: variant-tracking
  version: 1.0.0

requirements:
  host:
    - python
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)

    variant_config = VariantConfig({"python": ["3.10"]})

    rendered = render_recipe(stage0, variant_config)
    stage1 = rendered[0].recipe()

    # used_variant should contain python
    used_variant = stage1.used_variant
    assert "python" in used_variant


def test_pipeline_context_preservation() -> None:
    """Test that context is preserved through the pipeline."""
    yaml_content = """
context:
  my_var: custom_value
  version: 2.0.0

package:
  name: context-package
  version: ${{ version }}
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)

    # Check Stage0 context
    stage0_context = stage0.context

    assert isinstance(stage0, SingleOutputRecipe)
    assert "my_var" in stage0_context
    assert stage0_context["my_var"] == "custom_value"

    # Render and check Stage1 context
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)
    stage1 = rendered[0].recipe()

    stage1_context = stage1.context
    assert "my_var" in stage1_context
    assert stage1_context["my_var"] == "custom_value"
    assert stage1.package.version == "2.0.0"


def test_pipeline_multiple_renders() -> None:
    """Test rendering the same Stage0 recipe multiple times."""
    yaml_content = """
package:
  name: reusable-recipe
  version: 1.0.0
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()

    # Render multiple times
    rendered1 = render_recipe(stage0, variant_config)
    rendered2 = render_recipe(stage0, variant_config)

    # Both should work
    assert len(rendered1) == 1
    assert len(rendered2) == 1

    assert rendered1[0].recipe().package.name == "reusable-recipe"
    assert rendered2[0].recipe().package.name == "reusable-recipe"


def test_pipeline_from_dict_error_missing_package() -> None:
    """Test that creating recipe from dict with missing package field gives good error."""
    recipe_dict = {"build": {"number": 0}}

    with pytest.raises(Exception) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value).lower()
    assert "failed to deserialize recipe" in error_msg or "missing" in error_msg


def test_pipeline_from_dict_error_wrong_type() -> None:
    """Test that creating recipe from dict accepts numeric version (gets converted to string)."""
    # Note: Version 123 gets converted to "123" automatically by serde
    # This is actually valid behavior
    recipe_dict = {"package": {"name": "test", "version": 123}}

    # This should actually work - numeric values get stringified
    recipe = Stage0Recipe.from_dict(recipe_dict)
    assert recipe is not None


def test_pipeline_from_dict_error_invalid_structure() -> None:
    """Test that creating recipe from dict with invalid structure gives good error."""
    recipe_dict = {"invalid_key": "value", "another_invalid": 123}

    with pytest.raises(Exception) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # The error message contains debug format, check for "missing" or "package" field
    assert "missing" in error_msg.lower() or "package" in error_msg.lower()


def test_pipeline_stage1_requirements_detail() -> None:
    """Test detailed access to Stage1 requirements lists."""
    yaml_content = """
package:
  name: req-detail-test
  version: 1.0.0

requirements:
  build:
    - cmake
    - gcc
  host:
    - python >=3.8
    - numpy
  run:
    - python >=3.8
    - numpy
    - pandas
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    stage1 = rendered[0].recipe()
    reqs = stage1.requirements

    # Test that requirements are accessible as lists
    build_reqs = reqs.build
    assert isinstance(build_reqs, list)
    assert len(build_reqs) >= 2

    host_reqs = reqs.host
    assert isinstance(host_reqs, list)
    assert len(host_reqs) >= 2

    run_reqs = reqs.run
    assert isinstance(run_reqs, list)
    assert len(run_reqs) >= 3


def test_pipeline_stage1_about_detail() -> None:
    """Test detailed access to Stage1 about metadata."""
    yaml_content = """
package:
  name: about-detail-test
  version: 1.0.0

about:
  summary: This is a detailed test
  license: Apache-2.0
  homepage: https://example.com
  repository: https://github.com/example/repo
  documentation: https://docs.example.com
  description: |
    A longer description
    that spans multiple lines.
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    stage1 = rendered[0].recipe()
    about = stage1.about

    assert about.summary == "This is a detailed test"
    assert about.license == "Apache-2.0"
    # URLs may have trailing slash added
    assert "https://example.com" in about.homepage
    assert "https://github.com/example/repo" in about.repository
    assert "https://docs.example.com" in about.documentation
    assert about.description is not None
    assert "longer description" in about.description


def test_pipeline_stage1_build_properties() -> None:
    """Test detailed access to Stage1 build properties."""
    yaml_content = """
package:
  name: build-detail-test
  version: 1.0.0

build:
  number: 42
  string: custom_string
  script:
    - echo "Building"
    - make install
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    stage1 = rendered[0].recipe()
    build = stage1.build

    assert build.number == 42
    # Note: build.string may be evaluated differently in Stage1
    assert build.string is not None


def test_pipeline_stage1_sources() -> None:
    """Test that Stage1 sources list is accessible."""
    yaml_content = """
package:
  name: source-test
  version: 1.0.0

source:
  url: https://example.com/package.tar.gz
  sha256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    stage1 = rendered[0].recipe()

    # sources should be a list
    sources = stage1.sources
    assert isinstance(sources, list)


def test_pipeline_rendered_variant_repr() -> None:
    """Test that RenderedVariant has a useful repr."""
    yaml_content = """
package:
  name: repr-test
  version: 1.0.0
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    variant = rendered[0]
    repr_str = repr(variant)

    assert "RenderedVariant" in repr_str
    assert "repr-test" in repr_str


def test_pipeline_stage1_to_dict_comprehensive() -> None:
    """Test that Stage1 to_dict produces a comprehensive dictionary."""
    yaml_content = """
package:
  name: comprehensive-test
  version: 1.0.0

build:
  number: 5

requirements:
  host:
    - python
  run:
    - python

about:
  summary: Test
  license: MIT
"""

    stage0 = Stage0Recipe.from_yaml(yaml_content)
    variant_config = VariantConfig()
    rendered = render_recipe(stage0, variant_config)

    stage1 = rendered[0].recipe()
    stage1_dict = stage1.to_dict()

    # Verify it's a proper dictionary with expected keys
    assert isinstance(stage1_dict, dict)
    assert "package" in stage1_dict
    assert "build" in stage1_dict
    assert "requirements" in stage1_dict
    assert "about" in stage1_dict

    # Verify nested structure
    assert "name" in stage1_dict["package"]
    assert "version" in stage1_dict["package"]
    assert stage1_dict["package"]["name"] == "comprehensive-test"
