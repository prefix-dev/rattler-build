"""
Platform-related types for rattler-build.

This module provides Pythonic APIs for platform detection, virtual packages, and
platform-specific configuration.
"""

from . import rattler_build as _rb

# Re-export the Rust types
Platform = _rb.platform_types.Platform
PlatformWithVirtualPackages = _rb.platform_types.PlatformWithVirtualPackages

__all__ = ["Platform", "PlatformWithVirtualPackages"]
