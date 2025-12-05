"""Tests for JinjaConfig (JinjaConfig) Python bindings."""

import pytest

from rattler_build import JinjaConfig, PlatformConfig, PlatformParseError


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


def test_selector_config_invalid_platform() -> None:
    """Test that invalid platforms raise PlatformParseError."""
    with pytest.raises(PlatformParseError, match="'invalid-platform' is not a known platform."):
        platform_config = PlatformConfig("invalid-platform")
        JinjaConfig(platform=platform_config)
