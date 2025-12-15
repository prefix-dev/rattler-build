"""
Tests for error handling and type validation in the recipe pipeline.

These tests ensure that we get clear, helpful error messages when things go wrong.
"""

from pathlib import Path

import pytest

from rattler_build import RattlerBuildError, RecipeParseError, Stage0Recipe


def test_from_dict_missing_required_field() -> None:
    """Test that from_dict gives clear error for missing required fields."""
    # Missing 'name' in package
    recipe_dict = {
        "package": {
            "version": "1.0.0"
            # missing 'name'
        }
    }

    with pytest.raises(RecipeParseError, match="missing required field 'name'"):
        Stage0Recipe.from_dict(recipe_dict)


def test_from_dict_invalid_build_number() -> None:
    """Test that from_dict validates build number types."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "build": {
            "number": "not a number"  # Should be int
        },
    }

    with pytest.raises(RecipeParseError, match=r"invalid value for 'build\.number'"):
        Stage0Recipe.from_dict(recipe_dict)


def test_from_dict_unknown_top_level_field() -> None:
    """Test error handling for unknown top-level fields."""
    recipe_dict = {"package": {"name": "test-package", "version": "1.0.0"}, "unknown_field": "should cause error"}

    with pytest.raises(RecipeParseError, match="unknown top-level field 'unknown_field'"):
        Stage0Recipe.from_dict(recipe_dict)


def test_from_dict_invalid_requirements_structure() -> None:
    """Test error handling for invalid requirements structure."""
    recipe_dict = {
        "package": {"name": "test-package", "version": "1.0.0"},
        "requirements": {
            "run": "should be a list"  # Should be list, not string
        },
    }

    with pytest.raises(RecipeParseError, match="expected sequence"):
        Stage0Recipe.from_dict(recipe_dict)


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

    with pytest.raises(RecipeParseError, match="missing required field 'package'"):
        Stage0Recipe.from_yaml(yaml_without_package)


def test_from_dict_empty_dict() -> None:
    """Test error handling for empty dictionary."""
    with pytest.raises(RecipeParseError, match="missing required field 'package'"):
        Stage0Recipe.from_dict({})


def test_build_script_failure_error_message(tmp_path: Path) -> None:
    """Test that build script failures provide helpful error messages."""
    # Create a recipe with a failing build script (typo in command)
    recipe_yaml = """
recipe:
  name: test-build-failure
  version: 1.0.0

outputs:
  - package:
      name: test-build-failure
      version: 1.0.0

    build:
      script:
        - ech "This should fail because 'ech' is not a valid command"
"""

    # Build up the Stage0 Recipe from YAML
    recipe = Stage0Recipe.from_yaml(recipe_yaml)
    output_dir = tmp_path / "output"

    # Expect a RattlerBuildError with helpful message
    with pytest.raises(RattlerBuildError) as exc_info:
        recipe.run_build(output_dir=output_dir)

    error_msg = str(exc_info.value)

    # The error should mention:
    # 1. That the command failed
    # 2. The command that failed (ech)
    # 3. Some context about what went wrong (exit code, stderr, etc.)

    # Check for the failed command in the error message
    assert "ech" in error_msg, f"Error message doesn't mention the failed command 'ech': {error_msg}"

    # Check for some indication of what went wrong (exit code or error details)
    assert any(
        keyword in error_msg.lower() for keyword in ["exit", "status", "code", "not found", "stderr"]
    ), f"Error message doesn't contain error details like exit code or stderr: {error_msg}"
