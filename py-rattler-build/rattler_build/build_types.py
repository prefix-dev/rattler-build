"""
Build-related types for rattler-build.

This module provides Pythonic APIs for build directories, packaging settings, and other
build-related configuration.
"""

from . import rattler_build as _rb

# Re-export the Rust types
Directories = _rb.build_types.Directories
PackagingSettings = _rb.build_types.PackagingSettings

__all__ = ["Directories", "PackagingSettings"]
