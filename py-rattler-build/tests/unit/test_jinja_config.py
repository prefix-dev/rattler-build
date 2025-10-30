"""Tests for JinjaConfig (JinjaConfig) Python bindings."""

import pytest
from rattler_build.jinja_config import JinjaConfig


def test_jinja_config_creation() -> None:
    """Test creating a JinjaConfig with default values."""
    config = JinjaConfig()

    assert config.target_platform is not None
    assert config.host_platform is not None
    assert config.build_platform is not None
    assert isinstance(config.experimental, bool)
    assert isinstance(config.allow_undefined, bool)


def test_jinja_config_with_platforms() -> None:
    """Test creating a JinjaConfig with specific platforms."""
    config = JinjaConfig(target_platform="linux-64", host_platform="linux-64", build_platform="linux-64")

    assert config.target_platform == "linux-64"
    assert config.host_platform == "linux-64"
    assert config.build_platform == "linux-64"


def test_jinja_config_platform_setters() -> None:
    """Test setting platforms after creation."""
    config = JinjaConfig()

    config.target_platform = "win-64"
    assert config.target_platform == "win-64"

    config.host_platform = "osx-arm64"
    assert config.host_platform == "osx-arm64"

    config.build_platform = "linux-aarch64"
    assert config.build_platform == "linux-aarch64"


def test_selector_config_experimental() -> None:
    """Test experimental flag."""
    config = JinjaConfig(experimental=True)
    assert config.experimental is True

    config.experimental = False
    assert config.experimental is False


def test_selector_config_allow_undefined() -> None:
    """Test allow_undefined flag."""
    config = JinjaConfig(allow_undefined=True)
    assert config.allow_undefined is True

    config.allow_undefined = False
    assert config.allow_undefined is False


def test_selector_config_with_variant() -> None:
    """Test creating a JinjaConfig with variant."""
    variant = {"python": "3.11", "numpy": "1.21"}
    config = JinjaConfig(variant=variant)

    assert config.variant is not None
    # Check that variant was set (exact structure depends on implementation)
    assert isinstance(config.variant, dict)


def test_selector_config_variant_setter() -> None:
    """Test setting variant after creation."""
    config = JinjaConfig()

    variant = {"python": "3.10", "build_number": 1}
    config.variant = variant

    assert config.variant is not None
    assert isinstance(config.variant, dict)


def test_selector_config_config_property() -> None:
    """Test that the config property returns the underlying PyJinjaConfig."""
    config = JinjaConfig(target_platform="linux-64")

    # The config property should return the internal _config
    underlying_config = config.config
    assert underlying_config is not None

    # It should be the same object
    assert underlying_config is config._config


def test_selector_config_repr() -> None:
    """Test the string representation."""
    config = JinjaConfig(target_platform="osx-64")
    repr_str = repr(config)

    assert "JinjaConfig" in repr_str
    assert "osx-64" in repr_str


def test_selector_config_invalid_platform() -> None:
    """Test that invalid platforms are rejected."""
    with pytest.raises(Exception):  # Should raise some error
        JinjaConfig(target_platform="invalid-platform-name-12345")
