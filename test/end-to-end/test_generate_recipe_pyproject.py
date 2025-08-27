"""
End-to-end tests for pyproject.toml recipe generation functionality.
"""

from pathlib import Path
from textwrap import dedent

import yaml

from helpers import RattlerBuild


def test_basic_pyproject_generation(rattler_build: RattlerBuild, tmp_path: Path):
    """Test basic pyproject.toml recipe generation."""
    # Create a simple pyproject.toml
    pyproject_content = dedent("""
        [build-system]
        requires = ["setuptools>=45", "wheel"]
        build-backend = "setuptools.build_meta"

        [project]
        name = "test-package"
        version = "1.0.0"
        description = "A test package"
        authors = [{name = "Test Author", email = "test@example.com"}]
        license = {text = "MIT"}
        readme = "README.md"
        requires-python = ">=3.8"
        dependencies = [
            "numpy>=1.20.0",
            "requests",
        ]
        keywords = ["test", "example"]
        classifiers = [
            "Development Status :: 4 - Beta",
            "Programming Language :: Python :: 3",
        ]

        [project.urls]
        Homepage = "https://github.com/example/test-package"
        Repository = "https://github.com/example/test-package.git"
    """).strip()

    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text(pyproject_content)

    # Generate recipe
    output_dir = tmp_path / "output"
    output_dir.mkdir()

    result = rattler_build(
        "generate-recipe",
        "pyproject",
        "--input",
        str(pyproject_file),
        "--output",
        str(output_dir / "recipe.yaml"),
        "--write",
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Check that recipe.yaml was created
    recipe_file = output_dir / "recipe.yaml"
    assert recipe_file.exists()

    # Parse and validate the generated recipe
    with open(recipe_file) as f:
        recipe = yaml.safe_load(f)

    # Validate basic structure
    assert recipe["schema_version"] == 1
    assert recipe["context"]["name"] == "test-package"
    assert recipe["context"]["version"] == "1.0.0"
    assert recipe["context"]["python_min"] == "3.8"
    assert recipe["package"]["name"] == "${{ name }}"
    assert recipe["package"]["version"] == "${{ version }}"
    assert recipe["about"]["summary"] == "A test package"
    assert recipe["about"]["license"] == "MIT"
    assert recipe["about"]["homepage"] == "https://github.com/example/test-package"
    assert (
        recipe["about"]["repository"] == "https://github.com/example/test-package.git"
    )

    # Check requirements
    assert "python >=3.8" in recipe["requirements"]["host"]
    assert "python >=3.8" in recipe["requirements"]["run"]
    assert "numpy >=1.20.0" in recipe["requirements"]["run"]
    assert "requests" in recipe["requirements"]["run"]
    assert "setuptools>=45" in recipe["requirements"]["host"]
    assert "wheel" in recipe["requirements"]["host"]

    # Check source
    assert (
        recipe["source"][0]["url"]
        == "https://github.com/example/test-package/archive/v${{ version }}.tar.gz"
    )


def test_pyproject_help_command(rattler_build: RattlerBuild):
    """Test that pyproject help is available."""
    result = rattler_build(
        "generate-recipe", "pyproject", "--help", capture_output=True, text=True
    )

    assert result.returncode == 0
    assert "pyproject.toml" in result.stdout.lower()


def test_pyproject_missing_file_error(rattler_build: RattlerBuild, tmp_path: Path):
    """Test error handling for missing pyproject.toml file."""
    nonexistent_file = tmp_path / "nonexistent.toml"

    result = rattler_build(
        "generate-recipe",
        "pyproject",
        "--input",
        str(nonexistent_file),
        "--output",
        str(tmp_path / "output.yaml"),
        "--write",
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "No such file" in result.stderr or "not found" in result.stderr.lower()


def test_pyproject_overwrite_protection(rattler_build: RattlerBuild, tmp_path: Path):
    """Test that existing files are not overwritten without --overwrite."""
    pyproject_content = dedent("""
        [project]
        name = "overwrite-test"
        version = "1.0.0"
    """).strip()

    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text(pyproject_content)

    recipe_file = tmp_path / "recipe.yaml"

    # Create existing recipe file
    recipe_file.write_text("existing: content")

    # Should fail without --overwrite
    result = rattler_build(
        "generate-recipe",
        "pyproject",
        "--input",
        str(pyproject_file),
        "--output",
        str(recipe_file),
        "--write",
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "already exists" in result.stderr or "overwrite" in result.stderr.lower()

    # Original content should be preserved
    assert recipe_file.read_text() == "existing: content"


def test_pyproject_overwrite_flag(rattler_build: RattlerBuild, tmp_path: Path):
    """Test that --overwrite allows overwriting existing files."""
    pyproject_content = dedent("""
        [project]
        name = "overwrite-test"
        version = "1.0.0"
    """).strip()

    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text(pyproject_content)

    recipe_file = tmp_path / "recipe.yaml"

    # Create existing recipe file
    recipe_file.write_text("existing: content")

    # Should succeed with --overwrite
    result = rattler_build(
        "generate-recipe",
        "pyproject",
        "--input",
        str(pyproject_file),
        "--output",
        str(recipe_file),
        "--write",
        "--overwrite",
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Should have new content
    with open(recipe_file) as f:
        recipe = yaml.safe_load(f)

    assert recipe["context"]["name"] == "overwrite-test"


def test_pyproject_schema_version_in_output(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Test that schema_version is correctly set in generated recipes."""
    pyproject_content = dedent("""
        [project]
        name = "schema-test"
        version = "1.0.0"
    """).strip()

    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text(pyproject_content)

    output_dir = tmp_path / "output"
    output_dir.mkdir()

    result = rattler_build(
        "generate-recipe",
        "pyproject",
        "--input",
        str(pyproject_file),
        "--output",
        str(output_dir / "recipe.yaml"),
        "--write",
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    recipe_file = output_dir / "recipe.yaml"

    # Check that the raw YAML starts with schema_version
    content = recipe_file.read_text()
    # Skip any comments at the top
    lines = [line for line in content.strip().split("\n") if not line.startswith("#")]
    assert lines[0] == "schema_version: 1"

    # Also verify it parses correctly
    with open(recipe_file) as f:
        recipe = yaml.safe_load(f)
    assert recipe["schema_version"] == 1
