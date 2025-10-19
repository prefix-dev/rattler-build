"""Test variant override functionality via CLI flags."""

from pathlib import Path
from helpers import RattlerBuild


def test_variant_override_single_value(rattler_build: RattlerBuild, tmp_path: Path):
    """Test single variant override with --variant flag."""
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    # Create a simple recipe that uses variants
    recipe_content = """
package:
  name: test-variant
  version: 1.0.0

build:
  string: py${{ python | replace(".", "") }}h${{ hash }}_${{ build_number }}
  number: 0

requirements:
  host:
    - python ${{ python }}.*
  run:
    - python ${{ python }}.*

about:
  summary: Test recipe for variant override
"""

    (recipe_dir / "recipe.yaml").write_text(recipe_content)

    # Render with variant override
    output = rattler_build.render(
        recipe_dir, tmp_path, extra_args=["--variant", "python=3.12"]
    )

    assert len(output) == 1
    recipe = output[0]["recipe"]

    # Check that Python version was set correctly
    assert "python 3.12.*" in recipe["requirements"]["host"]
    assert "python 3.12.*" in recipe["requirements"]["run"]
    assert "py312" in recipe["build"]["string"]

    # Check the variant was applied
    variant = output[0]["build_configuration"]["variant"]
    assert variant["python"] == "3.12"


def test_variant_override_multiple_values(rattler_build: RattlerBuild, tmp_path: Path):
    """Test multiple values for same variant key."""
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    # Create a recipe that uses Python and NumPy variants
    recipe_content = """
package:
  name: test-multi-variant
  version: 1.0.0

build:
  string: np${{ numpy | replace(".", "") }}py${{ python | replace(".", "") }}h${{ hash }}_${{ build_number }}
  number: 0

requirements:
  host:
    - python ${{ python }}.*
    - numpy ${{ numpy }}.*
  run:
    - python ${{ python }}.*
    - numpy ${{ numpy }}.*

about:
  summary: Test recipe for multiple variant values
"""

    (recipe_dir / "recipe.yaml").write_text(recipe_content)

    # Render with multiple Python versions and single NumPy version
    output = rattler_build.render(
        recipe_dir,
        tmp_path,
        extra_args=["--variant", "python=3.11,3.12", "--variant", "numpy=2.0"],
    )

    # Should create 2 variants (one for each Python version)
    assert len(output) == 2

    # Check first variant (Python 3.11)
    recipe1 = output[0]["recipe"]
    variant1 = output[0]["build_configuration"]["variant"]
    assert "python 3.11.*" in recipe1["requirements"]["host"]
    assert "numpy 2.0.*" in recipe1["requirements"]["host"]
    assert variant1["python"] == "3.11"
    assert variant1["numpy"] == "2.0"
    assert "np20py311" in recipe1["build"]["string"]

    # Check second variant (Python 3.12)
    recipe2 = output[1]["recipe"]
    variant2 = output[1]["build_configuration"]["variant"]
    assert "python 3.12.*" in recipe2["requirements"]["host"]
    assert "numpy 2.0.*" in recipe2["requirements"]["host"]
    assert variant2["python"] == "3.12"
    assert variant2["numpy"] == "2.0"
    assert "np20py312" in recipe2["build"]["string"]


def test_variant_override_with_file(rattler_build: RattlerBuild, tmp_path: Path):
    """Test that CLI variant overrides take precedence over variant config files."""
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    # Create a simple recipe
    recipe_content = """
package:
  name: test-variant-precedence
  version: 1.0.0

build:
  string: py${{ python | replace(".", "") }}h${{ hash }}_${{ build_number }}
  number: 0

requirements:
  host:
    - python ${{ python }}.*
  run:
    - python ${{ python }}.*

about:
  summary: Test variant override precedence
"""

    (recipe_dir / "recipe.yaml").write_text(recipe_content)

    # Create a variant config file
    variant_config = tmp_path / "variant_config.yaml"
    variant_config.write_text("""
python:
  - "3.10"
  - "3.11"
""")

    # Render with both variant config file and CLI override
    output = rattler_build.render(
        recipe_dir,
        tmp_path,
        variant_config=variant_config,
        extra_args=["--variant", "python=3.12"],
    )

    # CLI override should take precedence
    assert len(output) == 1
    recipe = output[0]["recipe"]
    variant = output[0]["build_configuration"]["variant"]

    assert "python 3.12.*" in recipe["requirements"]["host"]
    assert variant["python"] == "3.12"
    assert "py312" in recipe["build"]["string"]


def test_variant_override_complex_values(rattler_build: RattlerBuild, tmp_path: Path):
    """Test variant override with multiple keys and multiple values."""
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    # Create a recipe with multiple variant keys
    recipe_content = """
package:
  name: test-complex-variant
  version: 1.0.0

build:
  string: np${{ numpy | replace(".", "") }}py${{ python | replace(".", "") }}${{ blas_impl }}h${{ hash }}_${{ build_number }}
  number: 0

requirements:
  host:
    - python ${{ python }}.*
    - numpy ${{ numpy }}.*
    - ${{ blas_impl }}
  run:
    - python ${{ python }}.*
    - numpy ${{ numpy }}.*
    - ${{ blas_impl }}

about:
  summary: Test complex variant combinations
"""

    (recipe_dir / "recipe.yaml").write_text(recipe_content)

    # Render with multiple variant overrides
    output = rattler_build.render(
        recipe_dir,
        tmp_path,
        extra_args=[
            "--variant",
            "python=3.11,3.12",
            "--variant",
            "numpy=2.0,2.1",
            "--variant",
            "blas_impl=openblas",
        ],
    )

    # Should create 4 variants (2 Python * 2 NumPy * 1 BLAS)
    assert len(output) == 4

    # Collect all combinations
    combinations = []
    for item in output:
        variant = item["build_configuration"]["variant"]
        combinations.append((variant["python"], variant["numpy"], variant["blas_impl"]))

    # Check all expected combinations exist
    expected = [
        ("3.11", "2.0", "openblas"),
        ("3.11", "2.1", "openblas"),
        ("3.12", "2.0", "openblas"),
        ("3.12", "2.1", "openblas"),
    ]

    assert sorted(combinations) == sorted(expected)
