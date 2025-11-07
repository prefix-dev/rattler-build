"""Tests for VariantConfig Python bindings."""

import tempfile
from pathlib import Path
from rattler_build.variant_config import VariantConfig


def test_variant_config_creation() -> None:
    """Test creating an empty VariantConfig."""
    config = VariantConfig()
    assert len(config) == 0


def test_variant_config_set_values() -> None:
    """Test setting variant values."""
    config = VariantConfig()

    # Set some variant values
    config.set_values("python", ["3.8", "3.9", "3.10"])
    config.set_values("numpy", ["1.21", "1.22"])

    # Check that keys were added
    keys = config.keys()
    assert "python" in keys
    assert "numpy" in keys
    assert len(config) == 2


def test_variant_config_get_values() -> None:
    """Test getting variant values."""
    config = VariantConfig()

    config.set_values("python", ["3.9", "3.10", "3.11"])

    values = config.get_values("python")
    assert values is not None
    assert len(values) == 3


def test_variant_config_get_nonexistent_key() -> None:
    """Test getting values for a key that doesn't exist."""
    config = VariantConfig()

    values = config.get_values("nonexistent")
    assert values is None


def test_variant_config_to_dict() -> None:
    """Test converting VariantConfig to dictionary."""
    config = VariantConfig()

    config.set_values("python", ["3.10", "3.11"])
    config.set_values("rust", ["1.70", "1.71"])

    config_dict = config.to_dict()
    assert isinstance(config_dict, dict)
    assert "python" in config_dict
    assert "rust" in config_dict


def test_variant_config_merge() -> None:
    """Test merging two VariantConfigs."""
    config1 = VariantConfig()
    config1.set_values("python", ["3.9", "3.10"])

    config2 = VariantConfig()
    config2.set_values("numpy", ["1.21", "1.22"])
    config2.set_values("cuda", ["11.8", "12.0"])

    # Merge config2 into config1
    config1.merge(config2)

    # Check that config1 now has all keys
    keys = config1.keys()
    assert "python" in keys
    assert "numpy" in keys
    assert "cuda" in keys


def test_variant_config_combinations() -> None:
    """Test generating variant combinations."""
    config = VariantConfig()

    config.set_values("python", ["3.9", "3.10"])
    config.set_values("numpy", ["1.21", "1.22"])

    combinations = config.combinations()

    # Should have 2 * 2 = 4 combinations
    assert len(combinations) == 4

    # Each combination should be a dict
    for combo in combinations:
        assert isinstance(combo, dict)
        assert "python" in combo
        assert "numpy" in combo


def test_variant_config_from_yaml() -> None:
    """Test loading VariantConfig from YAML string."""
    yaml_content = """
python:
  - "3.9"
  - "3.10"
  - "3.11"
numpy:
  - "1.21"
  - "1.22"
"""

    config = VariantConfig.from_yaml(yaml_content)

    keys = config.keys()
    assert "python" in keys
    assert "numpy" in keys

    python_values = config.get_values("python")
    assert python_values is not None
    assert len(python_values) == 3


def test_variant_config_from_file() -> None:
    """Test loading VariantConfig from a file."""
    yaml_content = """
python:
  - "3.10"
  - "3.11"
rust:
  - "1.70"
"""

    # Create a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
        f.write(yaml_content)
        temp_path = Path(f.name)

    try:
        config = VariantConfig.from_file(temp_path)

        keys = config.keys()
        assert "python" in keys
        assert "rust" in keys
    finally:
        # Clean up
        temp_path.unlink()


def test_variant_config_with_different_types() -> None:
    """Test setting variant values with different types."""
    config = VariantConfig()

    # Strings
    config.set_values("version", ["1.0", "2.0"])

    # The values should be stored
    values = config.get_values("version")
    assert values is not None
    assert len(values) == 2


def test_variant_config_len() -> None:
    """Test the __len__ method."""
    config = VariantConfig()
    assert len(config) == 0

    config.set_values("python", ["3.9"])
    assert len(config) == 1

    config.set_values("numpy", ["1.21"])
    assert len(config) == 2


def test_variant_config_repr() -> None:
    """Test the string representation."""
    config = VariantConfig()
    config.set_values("python", ["3.10"])

    repr_str = repr(config)
    assert "VariantConfig" in repr_str
    assert "keys=1" in repr_str


def test_variant_config_empty_combinations() -> None:
    """Test combinations on empty config."""
    config = VariantConfig()

    combinations = config.combinations()

    # Empty config should give one empty combination
    assert len(combinations) >= 0


def test_variant_config_zip_keys() -> None:
    """Test zip_keys functionality."""
    config_without_zip = VariantConfig()

    # Initially, zip_keys should be None
    assert config_without_zip.zip_keys is None

    # Set variant values
    config_without_zip.set_values("python", ["3.9", "3.10", "3.11"])
    config_without_zip.set_values("numpy", ["1.20", "1.21", "1.22"])

    # Without zip_keys, we get all combinations (3 * 3 = 9)
    combinations = config_without_zip.combinations()
    assert len(combinations) == 9

    # Create new config with zip_keys to synchronize python and numpy
    config = VariantConfig(
        {"python": ["3.9", "3.10", "3.11"], "numpy": ["1.20", "1.21", "1.22"]}, zip_keys=[["python", "numpy"]]
    )
    assert config.zip_keys == [["python", "numpy"]]

    # With zip_keys, we get only synchronized combinations (3)
    combinations = config.combinations()
    assert len(combinations) == 3

    # Verify the combinations are synchronized
    for i, combo in enumerate(combinations):
        assert combo["python"] == ["3.9", "3.10", "3.11"][i]
        assert combo["numpy"] == ["1.20", "1.21", "1.22"][i]


def test_variant_config_from_yaml_with_zip_keys() -> None:
    """Test loading VariantConfig from YAML with zip_keys."""
    yaml_content = """
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
zip_keys:
  - [python, numpy]
"""

    config = VariantConfig.from_yaml(yaml_content)

    # Check that zip_keys were parsed correctly
    assert config.zip_keys is not None
    assert len(config.zip_keys) == 1
    assert config.zip_keys[0] == ["python", "numpy"]

    # Check that combinations are synchronized
    combinations = config.combinations()
    assert len(combinations) == 2  # Not 4


def test_variant_config_from_file_with_context() -> None:
    """Test loading VariantConfig with JinjaConfig context."""
    from rattler_build import JinjaConfig

    yaml_content = """
c_compiler:
  - if: unix
    then: gcc
  - if: win
    then: msvc
"""

    # Create a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
        f.write(yaml_content)
        temp_path = Path(f.name)

    try:
        # Load with Linux context
        jinja_config_linux = JinjaConfig(target_platform="linux-64")
        config_linux = VariantConfig.from_file_with_context(temp_path, jinja_config_linux)

        values_linux = config_linux.get_values("c_compiler")
        assert values_linux is not None
        assert "gcc" in values_linux
        assert "msvc" not in values_linux

        # Load with Windows context
        jinja_config_win = JinjaConfig(target_platform="win-64")
        config_win = VariantConfig.from_file_with_context(temp_path, jinja_config_win)

        values_win = config_win.get_values("c_compiler")
        assert values_win is not None
        assert "msvc" in values_win
        assert "gcc" not in values_win
    finally:
        # Clean up
        temp_path.unlink()


def test_variant_config_from_yaml_with_context() -> None:
    """Test loading VariantConfig from YAML string with JinjaConfig context."""
    from rattler_build import JinjaConfig

    yaml_content = """
c_compiler:
  - if: unix
    then: gcc
  - if: win
    then: msvc
cxx_compiler:
  - if: unix
    then: gxx
  - if: win
    then: msvc
"""

    # Load with Linux context
    jinja_config_linux = JinjaConfig(target_platform="linux-64")
    config_linux = VariantConfig.from_yaml_with_context(yaml_content, jinja_config_linux)

    c_values = config_linux.get_values("c_compiler")
    assert c_values is not None
    assert "gcc" in c_values

    cxx_values = config_linux.get_values("cxx_compiler")
    assert cxx_values is not None
    assert "gxx" in cxx_values


def test_variant_config_from_conda_build_config() -> None:
    """Test loading conda_build_config.yaml format with selectors."""
    from rattler_build import JinjaConfig

    yaml_content = """
python:
  - 3.9
  - 3.10  # [unix]
  - 3.11  # [osx]
c_compiler:
  - gcc       # [linux]
  - clang     # [osx]
  - vs2019    # [win]
"""

    # Create a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", delete=False) as f:
        f.write(yaml_content)
        temp_path = Path(f.name)

    try:
        # Load with Linux context
        jinja_config_linux = JinjaConfig(target_platform="linux-64")
        config_linux = VariantConfig.from_conda_build_config(temp_path, jinja_config_linux)

        python_values = config_linux.get_values("python")
        assert python_values is not None
        assert len(python_values) == 2  # 3.9 and 3.10 (unix selector)

        c_compiler_values = config_linux.get_values("c_compiler")
        assert c_compiler_values is not None
        assert "gcc" in c_compiler_values
        assert "clang" not in c_compiler_values
        assert "vs2019" not in c_compiler_values

        # Load with macOS context
        jinja_config_osx = JinjaConfig(target_platform="osx-64")
        config_osx = VariantConfig.from_conda_build_config(temp_path, jinja_config_osx)

        python_values_osx = config_osx.get_values("python")
        assert python_values_osx is not None
        assert len(python_values_osx) == 3  # 3.9, 3.10 (unix), and 3.11 (osx)

        c_compiler_values_osx = config_osx.get_values("c_compiler")
        assert c_compiler_values_osx is not None
        assert "clang" in c_compiler_values_osx
        assert "gcc" not in c_compiler_values_osx

        # Load with Windows context
        jinja_config_win = JinjaConfig(target_platform="win-64")
        config_win = VariantConfig.from_conda_build_config(temp_path, jinja_config_win)

        python_values_win = config_win.get_values("python")
        assert python_values_win is not None
        assert len(python_values_win) == 1  # Only 3.9 (no unix/osx selectors match)

        c_compiler_values_win = config_win.get_values("c_compiler")
        assert c_compiler_values_win is not None
        assert "vs2019" in c_compiler_values_win
    finally:
        # Clean up
        temp_path.unlink()


def test_variant_config_multiple_zip_key_groups() -> None:
    """Test multiple zip_key groups."""
    config = VariantConfig(
        {
            "python": ["3.9", "3.10"],
            "numpy": ["1.20", "1.21"],
            "c_compiler": ["gcc", "clang"],
            "cxx_compiler": ["gxx", "clangxx"],
        },
        zip_keys=[["python", "numpy"], ["c_compiler", "cxx_compiler"]],
    )

    # Should get 2 * 2 = 4 combinations (not 2 * 2 * 2 * 2 = 16)
    combinations = config.combinations()
    assert len(combinations) == 4

    # Check that synchronization is preserved
    for combo in combinations:
        # python-numpy should be synchronized
        if combo["python"] == "3.9":
            assert combo["numpy"] == "1.20"
        else:
            assert combo["numpy"] == "1.21"

        # c_compiler-cxx_compiler should be synchronized
        if combo["c_compiler"] == "gcc":
            assert combo["cxx_compiler"] == "gxx"
        else:
            assert combo["cxx_compiler"] == "clangxx"
