"""Tests for render module typed structures (HashInfo and PinSubpackageInfo)."""

from rattler_build import RenderConfig, Stage0Recipe, VariantConfig


def test_hash_info_type() -> None:
    """Test that HashInfo is a proper typed structure."""
    # Create a simple recipe
    recipe_yaml = """
schema_version: 1
package:
  name: test-hash
  version: "1.0.0"
"""

    # Parse recipe
    recipe = Stage0Recipe.from_yaml(recipe_yaml)

    # Create empty variant config
    variant_config = VariantConfig()

    # Render recipe
    render_config = RenderConfig()
    rendered = recipe.render(variant_config, render_config)

    assert len(rendered) == 1
    variant = rendered[0]

    # Test hash_info - it always exists now
    hash_info = variant.hash_info()

    assert hash_info is not None
    # Check that we can access properties like a typed object
    assert hasattr(hash_info, "hash")
    assert hasattr(hash_info, "prefix")
    assert isinstance(hash_info.hash, str)
    assert isinstance(hash_info.prefix, str)
    # Hash should be 7 characters
    assert len(hash_info.hash) == 7


def test_pin_subpackages_type() -> None:
    """Test that PinSubpackageInfo is a proper typed structure."""
    # Create a recipe with multiple outputs and pin_subpackage
    recipe_yaml = """
schema_version: 1
recipe:
  name: test-pin

outputs:
  - package:
      name: test-lib
      version: "1.0.0"

  - package:
      name: test-app
      version: "1.0.0"
    requirements:
      host:
        - ${{ pin_subpackage('test-lib', exact=True) }}
"""

    # Parse recipe
    recipe = Stage0Recipe.from_yaml(recipe_yaml)

    # Create empty variant config
    variant_config = VariantConfig()

    # Render recipe
    render_config = RenderConfig()
    rendered = recipe.render(variant_config, render_config)

    # Find the test-app output (should have pin_subpackages)
    app_variant = None
    for variant in rendered:
        recipe_obj = variant.recipe()
        # Access package property (not method)
        if recipe_obj.package.name == "test-app":
            app_variant = variant
            break

    assert app_variant is not None, "test-app variant not found"

    # Test pin_subpackages
    pin_subpackages = app_variant.pin_subpackages()

    assert isinstance(pin_subpackages, dict)

    # Check that we have the test-lib pin
    if "test-lib" in pin_subpackages:
        pin_info = pin_subpackages["test-lib"]

        # Check that we can access properties like a typed object
        assert hasattr(pin_info, "name")
        assert hasattr(pin_info, "version")
        assert hasattr(pin_info, "build_string")
        assert hasattr(pin_info, "exact")

        assert isinstance(pin_info.name, str)
        assert isinstance(pin_info.version, str)
        assert isinstance(pin_info.exact, bool)

        # Since we used exact=True, this should be True
        assert pin_info.exact is True
        assert pin_info.name == "test-lib"
        assert pin_info.version == "1.0.0"


def test_hash_info_repr() -> None:
    """Test the __repr__ of HashInfo."""
    recipe_yaml = """
schema_version: 1
package:
  name: test-repr
  version: "1.0.0"
"""

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    render_config = RenderConfig()
    rendered = recipe.render(variant_config, render_config)

    hash_info = rendered[0].hash_info()
    if hash_info is not None:
        repr_str = repr(hash_info)
        assert "HashInfo" in repr_str
        assert "hash=" in repr_str
        assert "prefix=" in repr_str


def test_pin_subpackage_info_repr() -> None:
    """Test the __repr__ of PinSubpackageInfo."""
    recipe_yaml = """
schema_version: 1
recipe:
  name: test-pin-repr

outputs:
  - package:
      name: lib
      version: "2.0.0"

  - package:
      name: app
      version: "2.0.0"
    requirements:
      host:
        - ${{ pin_subpackage('lib', upper_bound='x.x') }}
"""

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    render_config = RenderConfig()
    rendered = recipe.render(variant_config, render_config)

    # Find app output
    for variant in rendered:
        if variant.recipe().package.name == "app":
            pin_subpackages = variant.pin_subpackages()
            if "lib" in pin_subpackages:
                pin_info = pin_subpackages["lib"]
                repr_str = repr(pin_info)
                assert "PinSubpackageInfo" in repr_str
                assert "name=" in repr_str
                assert "version=" in repr_str
                assert "exact=" in repr_str
                break


def test_hash_info_always_present() -> None:
    """Test that hash_info is always present."""
    recipe_yaml = """
schema_version: 1
package:
  name: simple-package
  version: "1.0.0"
"""

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    render_config = RenderConfig()
    rendered = recipe.render(variant_config, render_config)

    hash_info = rendered[0].hash_info()
    # Hash info is always present
    assert hash_info is not None
    assert isinstance(hash_info.hash, str)
    assert len(hash_info.hash) == 7


def test_empty_pin_subpackages() -> None:
    """Test that pin_subpackages returns empty dict when no pins are present."""
    recipe_yaml = """
schema_version: 1
package:
  name: no-pins
  version: "1.0.0"
"""

    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    variant_config = VariantConfig()

    render_config = RenderConfig()
    rendered = recipe.render(variant_config, render_config)

    pin_subpackages = rendered[0].pin_subpackages()
    assert isinstance(pin_subpackages, dict)
    assert len(pin_subpackages) == 0
