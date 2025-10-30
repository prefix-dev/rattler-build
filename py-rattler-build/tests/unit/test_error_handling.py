"""
Tests for error handling and type validation in the recipe pipeline.

These tests ensure that we get clear, helpful error messages when things go wrong.
"""

import pytest
from rattler_build.stage0 import Recipe as Stage0Recipe, SingleOutputRecipe
from rattler_build.rattler_build import RecipeParseError


def test_from_dict_missing_required_field() -> None:
    """Test that from_dict gives clear error for missing required fields."""
    # Missing 'name' in package
    recipe_dict = {
        "package": {
            "version": "1.0.0"
            # missing 'name'
        }
    }

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    # Error should mention the missing field
    error_msg = str(exc_info.value)
    assert "name" in error_msg.lower() or "package" in error_msg.lower()


def test_from_dict_wrong_type_for_version() -> None:
    """Test that from_dict validates version types."""
    recipe_dict = {
        "package": {
            "name": "test-package",
            "version": 123,  # Should be string
        }
    }

    # This should either work (converting to string) or give a clear error
    # The behavior depends on the parser's strictness
    try:
        stage0 = Stage0Recipe.from_dict(recipe_dict)
        # If it works, version should be converted or accepted
        assert stage0 is not None
    except RecipeParseError as e:
        # If it fails, error should be clear
        assert "version" in str(e).lower() or "type" in str(e).lower()


def test_from_dict_invalid_build_number() -> None:
    """Test that from_dict validates build number types."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "build": {
            "number": "not a number"  # Should be int
        },
    }

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # Error should mention build or number or type mismatch
    assert any(word in error_msg.lower() for word in ["build", "number", "int", "type", "invalid"])


def test_from_dict_invalid_structure() -> None:
    """Test that from_dict rejects completely invalid structures."""
    # Not a dict at all
    with pytest.raises((RecipeParseError, TypeError, AttributeError)):
        Stage0Recipe.from_dict({"invalid_key": "value"})


def test_from_dict_unknown_top_level_field() -> None:
    """Test error handling for unknown top-level fields."""
    recipe_dict = {"package": {"name": "test-package", "version": "1.0.0"}, "unknown_field": "should cause error"}

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # Error should mention the unknown field
    assert "unknown" in error_msg.lower() or "unexpected" in error_msg.lower() or "unknown_field" in error_msg


def test_from_dict_invalid_requirements_structure() -> None:
    """Test error handling for invalid requirements structure."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "requirements": {
            "run": "should be a list"  # Should be list, not string
        },
    }

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # Error should mention sequence or type mismatch
    assert "sequence" in error_msg.lower() or "array" in error_msg.lower() or "type" in error_msg.lower()


def test_from_yaml_invalid_yaml_syntax() -> None:
    """Test error handling for invalid YAML syntax."""
    invalid_yaml = """
    package:
      name: test
      version: [unclosed bracket
    """

    with pytest.raises(RecipeParseError):
        Stage0Recipe.from_yaml(invalid_yaml)


def test_from_yaml_missing_package_section() -> None:
    """Test error handling when package section is missing."""
    yaml_without_package = """
build:
  number: 0
"""

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_yaml(yaml_without_package)

    error_msg = str(exc_info.value)
    # Should mention package is required
    assert "package" in error_msg.lower()


def test_from_dict_empty_dict() -> None:
    """Test error handling for empty dictionary."""
    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict({})

    error_msg = str(exc_info.value)
    assert "package" in error_msg.lower() or "required" in error_msg.lower()


def test_from_dict_nested_validation() -> None:
    """Test that nested field validation provides clear errors."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "build": {
            "number": 0,
            "python": {
                "entry_points": "should be a list"  # Should be list
            },
        },
    }

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # Error should mention sequence or type mismatch
    assert "sequence" in error_msg.lower() or "type" in error_msg.lower()


def test_from_dict_provides_helpful_message() -> None:
    """Test that from_dict accepts integer names and converts them."""
    recipe_dict = {
        "package": {
            "name": 123,  # Will be converted to string
            "version": "1.0.0",
        }
    }

    # Integer names are accepted and converted to strings
    stage0 = Stage0Recipe.from_dict(recipe_dict)
    assert stage0 is not None


def test_from_dict_list_of_strings_vs_object() -> None:
    """Test clear errors when expecting list but getting something else."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "requirements": {
            "host": {"not": "a list"}  # Should be list
        },
    }

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # Should indicate type mismatch (sequence expected)
    assert "sequence" in error_msg.lower() or "type" in error_msg.lower()


def test_error_includes_field_path() -> None:
    """Test that errors include the path to the problematic field."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "build": {
            "dynamic_linking": {
                "binary_relocation": "not a boolean"  # Should be bool
            }
        },
    }

    with pytest.raises(RecipeParseError) as exc_info:
        Stage0Recipe.from_dict(recipe_dict)

    error_msg = str(exc_info.value)
    # Error should help locate the problem
    # It might mention the field name or path
    assert any(
        word in error_msg.lower() for word in ["dynamic_linking", "binary_relocation", "build", "bool", "boolean"]
    )


def test_from_dict_valid_minimal_recipe() -> None:
    """Test that a minimal valid recipe works."""
    # This should NOT raise an error
    recipe_dict = {"package": {"name": "minimal-package", "version": "1.0.0"}}

    stage0 = Stage0Recipe.from_dict(recipe_dict)
    assert isinstance(stage0, SingleOutputRecipe)

def test_from_dict_with_schema_version() -> None:
    """Test that schema_version is accepted."""
    recipe_dict = {"schema_version": 1, "package": {"name": "versioned-package", "version": "1.0.0"}}

    stage0 = Stage0Recipe.from_dict(recipe_dict)
    assert stage0 is not None
