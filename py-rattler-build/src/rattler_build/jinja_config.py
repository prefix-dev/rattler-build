from pathlib import Path
from typing import Any

from rattler_build._rattler_build import PyJinjaConfig
from rattler_build.tool_config import PlatformConfig


class JinjaConfig:
    """Python wrapper for PyJinjaConfig to provide a cleaner interface.

    Args:
        platform: Platform configuration (target, build, host platforms, experimental flag, and recipe_path)
        allow_undefined: Whether to allow undefined variables in Jinja templates
        variant: Variant configuration dictionary

    Example:
        ```python
        from rattler_build.tool_config import PlatformConfig

        platform = PlatformConfig("linux-64")
        config = JinjaConfig(platform=platform)
        ```
    """

    _config: PyJinjaConfig
    platform: PlatformConfig | None

    def __init__(
        self,
        platform: PlatformConfig | None = None,
        allow_undefined: bool | None = None,
        variant: dict[str, Any] | None = None,
    ):
        self.platform = platform
        self._config = PyJinjaConfig(
            target_platform=platform.target_platform if platform else None,
            host_platform=platform.host_platform if platform else None,
            build_platform=platform.build_platform if platform else None,
            experimental=platform.experimental if platform else None,
            allow_undefined=allow_undefined,
            variant=variant,
            recipe_path=Path(platform.recipe_path) if (platform and platform.recipe_path) else None,
        )

    @property
    def target_platform(self) -> str | None:
        """Get the target platform."""
        return self._config.target_platform

    @property
    def host_platform(self) -> str | None:
        """Get the host platform."""
        return self._config.host_platform

    @property
    def build_platform(self) -> str | None:
        """Get the build platform."""
        return self._config.build_platform

    @property
    def experimental(self) -> bool | None:
        """Get whether experimental features are enabled."""
        return self._config.experimental

    @property
    def allow_undefined(self) -> bool | None:
        """Get whether undefined variables are allowed."""
        return self._config.allow_undefined

    @property
    def variant(self) -> dict[str, Any]:
        """Get the variant configuration."""
        return self._config.variant

    @property
    def recipe_path(self) -> str | None:
        """Get the recipe path."""
        path = self._config.recipe_path
        return str(path) if path else None

    def __repr__(self) -> str:
        return f"JinjaConfig(target_platform={self.target_platform!r}, variant={self.variant!r})"
