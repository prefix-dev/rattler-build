from typing import Any

from ._rattler_build import PyJinjaConfig


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

    def __repr__(self) -> str:
        return f"JinjaConfig(target_platform={self.target_platform!r}, variant={self.variant!r})"
