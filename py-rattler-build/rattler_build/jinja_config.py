from typing import Any
from .rattler_build import PyJinjaConfig


class JinjaConfig:
    """Python wrapper for PyJinjaConfig to provide a cleaner interface."""

    _config: PyJinjaConfig

    def __init__(
        self,
        target_platform: str | None = None,
        host_platform: str | None = None,
        build_platform: str | None = None,
        experimental: bool | None = None,
        allow_undefined: bool | None = None,
        variant: dict[str, Any] | None = None,
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
    def target_platform(self) -> str | None:
        """Get the target platform."""
        return self._config.target_platform

    @target_platform.setter
    def target_platform(self, value: str | None) -> None:
        """Set the target platform."""
        self._config.target_platform = value

    @property
    def host_platform(self) -> str | None:
        """Get the host platform."""
        return self._config.host_platform

    @host_platform.setter
    def host_platform(self, value: str | None) -> None:
        """Set the host platform."""
        self._config.host_platform = value

    @property
    def build_platform(self) -> str | None:
        """Get the build platform."""
        return self._config.build_platform

    @build_platform.setter
    def build_platform(self, value: str | None) -> None:
        """Set the build platform."""
        self._config.build_platform = value

    @property
    def experimental(self) -> bool | None:
        """Get whether experimental features are enabled."""
        return self._config.experimental

    @experimental.setter
    def experimental(self, value: bool | None) -> None:
        """Set whether experimental features are enabled."""
        self._config.experimental = value

    @property
    def allow_undefined(self) -> bool | None:
        """Get whether undefined variables are allowed."""
        return self._config.allow_undefined

    @allow_undefined.setter
    def allow_undefined(self, value: bool | None) -> None:
        """Set whether undefined variables are allowed."""
        self._config.allow_undefined = value

    @property
    def variant(self) -> dict[str, Any]:
        """Get the variant configuration."""
        return self._config.variant

    @variant.setter
    def variant(self, value: dict[str, Any]) -> None:
        """Set the variant configuration."""
        self._config.variant = value

    @property
    def config(self) -> PyJinjaConfig:
        """Get the underlying PyJinjaConfig (for backward compatibility)."""
        return self._config

    def __repr__(self) -> str:
        return f"JinjaConfig(target_platform={self.target_platform!r}, variant={self.variant!r})"
