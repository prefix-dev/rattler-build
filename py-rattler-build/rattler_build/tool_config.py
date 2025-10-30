"""
Tool configuration for rattler-build.

This module provides a Pythonic API for configuring the build tool.
"""


from . import rattler_build as _rb

# Re-export the Rust type
ToolConfiguration = _rb.tool_config.ToolConfiguration

__all__ = ["ToolConfiguration"]
