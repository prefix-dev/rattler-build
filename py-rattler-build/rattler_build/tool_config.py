"""
Tool configuration for rattler-build.

This module provides a Pythonic API for configuring the build tool.
"""

from pathlib import Path
from typing import List, Optional, Union

from . import rattler_build as _rb

# Re-export the Rust type
ToolConfiguration = _rb.tool_config.ToolConfiguration

__all__ = ["ToolConfiguration"]
