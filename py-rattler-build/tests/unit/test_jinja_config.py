"""Tests for JinjaConfig (JinjaConfig) Python bindings."""

import pytest

from rattler_build.jinja_config import JinjaConfig
from rattler_build.tool_config import PlatformConfig


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
    platform = PlatformConfig("linux-64")
    config = JinjaConfig(platform=platform)

    assert config.target_platform == "linux-64"
    assert config.host_platform == "linux-64"
    assert config.build_platform == "linux-64"


def test_jinja_config_platform_constructor() -> None:
    """Test creating JinjaConfig with specific platforms."""
    platform = PlatformConfig(
        target_platform="win-64",
        host_platform="osx-arm64",
        build_platform="linux-aarch64",
    )
    config = JinjaConfig(platform=platform)

    assert config.target_platform == "win-64"
    assert config.host_platform == "osx-arm64"
    assert config.build_platform == "linux-aarch64"


def test_selector_config_experimental() -> None:
    """Test experimental flag."""
    platform_true = PlatformConfig("linux-64", experimental=True)
    config_true = JinjaConfig(platform=platform_true)
    assert config_true.experimental is True

    platform_false = PlatformConfig("linux-64", experimental=False)
    config_false = JinjaConfig(platform=platform_false)
    assert config_false.experimental is False


def test_selector_config_allow_undefined() -> None:
    """Test allow_undefined flag."""
    config_true = JinjaConfig(allow_undefined=True)
    assert config_true.allow_undefined is True

    config_false = JinjaConfig(allow_undefined=False)
    assert config_false.allow_undefined is False


def test_selector_config_with_variant() -> None:
    """Test creating a JinjaConfig with variant."""
    variant = {"python": "3.11", "numpy": "1.21"}
    config = JinjaConfig(variant=variant)

    assert config.variant is not None
    # Check that variant was set (exact structure depends on implementation)
    assert isinstance(config.variant, dict)


def test_selector_config_variant_constructor() -> None:
    """Test creating JinjaConfig with variant."""
    variant = {"python": "3.10", "build_number": 1}
    config = JinjaConfig(variant=variant)

    assert config.variant is not None
    assert isinstance(config.variant, dict)


def test_selector_config_repr() -> None:
    """Test the string representation."""
    platform_config = PlatformConfig("osx-64")
    config = JinjaConfig(platform=platform_config)
    repr_str = repr(config)

    assert "JinjaConfig" in repr_str
    assert "osx-64" in repr_str


def test_selector_config_invalid_platform() -> None:
    """Test that invalid platforms are rejected."""
    with pytest.raises(Exception):  # Should raise some error
        platform_config = PlatformConfig("invalid-platform-name-12345")
        JinjaConfig(platform=platform_config)
