"""
Tests for error handling and type validation in the recipe pipeline.

These tests ensure that we get clear, helpful error messages when things go wrong.
"""

from pathlib import Path

import pytest

from rattler_build import BuildError, RattlerBuildError, RecipeParseError, Stage0Recipe


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

    # Expect a BuildError (subclass of RattlerBuildError) with structured log
    with pytest.raises(BuildError) as exc_info:
        recipe.run_build(output_dir=output_dir)

    err = exc_info.value

    # BuildError should also be catchable as RattlerBuildError
    assert isinstance(err, RattlerBuildError)

    # str(error) shows just the error message, not the full build log
    error_msg = str(err)
    assert "Script failed" in error_msg
    assert "Build log" not in error_msg
    assert "ech" not in error_msg

    # The .message attribute contains the error message
    assert hasattr(err, "message")
    assert "Script failed" in err.message

    # The .log attribute contains the captured build log as list[str]
    assert hasattr(err, "log")
    assert isinstance(err.log, list)

    # The log should contain build details including the failed command
    log_text = "\n".join(err.log)
    assert "ech" in log_text, f"Build log doesn't mention the failed command 'ech': {log_text}"
