from typing import Any, Dict, Optional
from .rattler_build import PyJinjaConfig


class JinjaConfig:
    """Python wrapper for PyJinjaConfig to provide a cleaner interface."""

    _config: PyJinjaConfig

    def __init__(
        self,
        target_platform: Optional[str] = None,
        host_platform: Optional[str] = None,
        build_platform: Optional[str] = None,
        experimental: Optional[bool] = None,
        allow_undefined: Optional[bool] = None,
        variant: Optional[Dict[str, Any]] = None,
    ):
        self._config = PyJinjaConfig(
            target_platform=target_platform,
            host_platform=host_platform,
            build_platform=build_platform,
            experimental=experimental,
            allow_undefined=allow_undefined,
            variant=variant,
        )

    @property
    def target_platform(self) -> Optional[str]:
        """Get the target platform."""
        return self._config.target_platform

    @target_platform.setter
    def target_platform(self, value: Optional[str]) -> None:
        """Set the target platform."""
        self._config.target_platform = value

    @property
    def host_platform(self) -> Optional[str]:
        """Get the host platform."""
        return self._config.host_platform

    @host_platform.setter
    def host_platform(self, value: Optional[str]) -> None:
        """Set the host platform."""
        self._config.host_platform = value

    @property
    def build_platform(self) -> Optional[str]:
        """Get the build platform."""
        return self._config.build_platform

    @build_platform.setter
    def build_platform(self, value: Optional[str]) -> None:
        """Set the build platform."""
        self._config.build_platform = value

    @property
    def experimental(self) -> Optional[bool]:
        """Get whether experimental features are enabled."""
        return self._config.experimental

    @experimental.setter
    def experimental(self, value: Optional[bool]) -> None:
        """Set whether experimental features are enabled."""
        self._config.experimental = value

    @property
    def allow_undefined(self) -> Optional[bool]:
        """Get whether undefined variables are allowed."""
        return self._config.allow_undefined

    @allow_undefined.setter
    def allow_undefined(self, value: Optional[bool]) -> None:
        """Set whether undefined variables are allowed."""
        self._config.allow_undefined = value

    @property
    def variant(self) -> Dict[str, Any]:
        """Get the variant configuration."""
        return self._config.variant

    @variant.setter
    def variant(self, value: Dict[str, Any]) -> None:
        """Set the variant configuration."""
        self._config.variant = value

    @property
    def config(self) -> PyJinjaConfig:
        """Get the underlying PyJinjaConfig (for backward compatibility)."""
        return self._config

    def __repr__(self) -> str:
        return f"JinjaConfig(target_platform={self.target_platform!r}, variant={self.variant!r})"
