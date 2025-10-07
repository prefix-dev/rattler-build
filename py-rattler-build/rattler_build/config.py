"""Configuration objects for rattler-build.

This module provides Python-friendly wrappers around the Rust configuration types,
including selector configuration, variant configuration, and build configuration.
"""

from pathlib import Path
from typing import Any, Dict, List, Optional, Union

from .rattler_build import (
    PyBuildConfiguration,
    PyOutput,
    PySelectorConfig,
    PyToolConfiguration,
    PyVariantConfig,
    parse_recipe_with_variants as _parse_recipe_with_variants,
)


class SelectorConfig:
    """Configuration for selectors used during recipe parsing.

    Selectors control how recipes are parsed for different platforms and variants.
    This includes target platform, host platform, build platform, and variant variables.

    Args:
        target_platform: The target platform (e.g., 'linux-64', 'osx-arm64', 'win-64').
                        Defaults to current platform.
        host_platform: The host platform (relevant for noarch packages).
                      Defaults to current platform.
        build_platform: The build platform. Defaults to current platform.
        variant: Dictionary of variant variables (e.g., {'python': '3.11', 'numpy': '1.19'}).
        experimental: Enable experimental features. Defaults to False.
        allow_undefined: Allow undefined variables in Jinja templates. Defaults to False.
        recipe_path: Path to the recipe file. Optional.
        hash: Build hash string. Optional.

    Example:
        >>> config = SelectorConfig(
        ...     target_platform="linux-64",
        ...     variant={"python": "3.11", "numpy": "1.19"}
        ... )
        >>> print(config.target_platform)
        'linux-64'
    """

    def __init__(
        self,
        target_platform: Optional[str] = None,
        host_platform: Optional[str] = None,
        build_platform: Optional[str] = None,
        variant: Optional[Dict[str, Any]] = None,
        experimental: Optional[bool] = None,
        allow_undefined: Optional[bool] = None,
        recipe_path: Optional[Union[str, Path]] = None,
        hash: Optional[str] = None,
    ):
        """Initialize a SelectorConfig."""
        recipe_path_obj = Path(recipe_path) if recipe_path else None
        self._inner = PySelectorConfig(
            target_platform=target_platform,
            host_platform=host_platform,
            build_platform=build_platform,
            variant=variant,
            experimental=experimental,
            allow_undefined=allow_undefined,
            recipe_path=recipe_path_obj,
            hash=hash,
        )

    @property
    def target_platform(self) -> str:
        """Get the target platform.

        Returns:
            The target platform string (e.g., 'linux-64').
        """
        return self._inner.target_platform

    @target_platform.setter
    def target_platform(self, value: str) -> None:
        """Set the target platform.

        Args:
            value: The target platform string.
        """
        self._inner.target_platform = value

    @property
    def host_platform(self) -> str:
        """Get the host platform.

        Returns:
            The host platform string.
        """
        return self._inner.host_platform

    @host_platform.setter
    def host_platform(self, value: str) -> None:
        """Set the host platform.

        Args:
            value: The host platform string.
        """
        self._inner.host_platform = value

    @property
    def build_platform(self) -> str:
        """Get the build platform.

        Returns:
            The build platform string.
        """
        return self._inner.build_platform

    @build_platform.setter
    def build_platform(self, value: str) -> None:
        """Set the build platform.

        Args:
            value: The build platform string.
        """
        self._inner.build_platform = value

    @property
    def experimental(self) -> bool:
        """Get whether experimental features are enabled.

        Returns:
            True if experimental features are enabled.
        """
        return self._inner.experimental

    @experimental.setter
    def experimental(self, value: bool) -> None:
        """Set whether experimental features are enabled.

        Args:
            value: Whether to enable experimental features.
        """
        self._inner.experimental = value

    @property
    def allow_undefined(self) -> bool:
        """Get whether undefined variables are allowed.

        Returns:
            True if undefined variables are allowed.
        """
        return self._inner.allow_undefined

    @allow_undefined.setter
    def allow_undefined(self, value: bool) -> None:
        """Set whether undefined variables are allowed.

        Args:
            value: Whether to allow undefined variables.
        """
        self._inner.allow_undefined = value

    @property
    def recipe_path(self) -> Optional[Path]:
        """Get the path to the recipe file.

        Returns:
            The recipe path or None.
        """
        return self._inner.recipe_path

    @recipe_path.setter
    def recipe_path(self, value: Optional[Union[str, Path]]) -> None:
        """Set the path to the recipe file.

        Args:
            value: The recipe path.
        """
        self._inner.recipe_path = Path(value) if value else None

    @property
    def hash(self) -> Optional[str]:
        """Get the build hash.

        Returns:
            The build hash or None.
        """
        return self._inner.hash

    @hash.setter
    def hash(self, value: Optional[str]) -> None:
        """Set the build hash.

        Args:
            value: The build hash.
        """
        self._inner.hash = value

    @property
    def variant(self) -> Dict[str, Any]:
        """Get the variant configuration.

        Returns:
            Dictionary of variant variables.
        """
        return self._inner.variant

    @variant.setter
    def variant(self, value: Dict[str, Any]) -> None:
        """Set the variant configuration.

        Args:
            value: Dictionary of variant variables.
        """
        self._inner.variant = value

    def __repr__(self) -> str:
        """Return string representation."""
        return repr(self._inner)

    @property
    def _config(self) -> PySelectorConfig:
        """Get the underlying PySelectorConfig (internal use)."""
        return self._inner


class VariantConfig:
    """Represents variant configuration loaded from YAML files.

    Variant configurations define build matrices for packages, specifying different
    combinations of dependencies and build options.

    Example:
        >>> config = VariantConfig.from_files(
        ...     ["variants.yaml"],
        ...     selector_config
        ... )
        >>> print(config.variants)
        {'python': ['3.9', '3.10', '3.11'], 'numpy': ['1.19', '1.20']}
    """

    def __init__(self, inner: PyVariantConfig):
        """Initialize from PyVariantConfig (internal use)."""
        self._inner = inner

    @classmethod
    def from_files(cls, files: List[Union[str, Path]], selector_config: SelectorConfig) -> "VariantConfig":
        """Load variant configuration from YAML files.

        Args:
            files: List of paths to variant configuration files.
            selector_config: Selector configuration for parsing.

        Returns:
            A VariantConfig instance.

        Raises:
            RattlerBuildError: If the variant files cannot be loaded.

        Example:
            >>> selector = SelectorConfig(target_platform="linux-64")
            >>> variants = VariantConfig.from_files(
            ...     ["variants.yaml", "conda_build_config.yaml"],
            ...     selector
            ... )
        """
        file_paths = [Path(f) for f in files]
        inner = PyVariantConfig.from_files(file_paths, selector_config._config)
        return cls(inner)

    @property
    def variants(self) -> Dict[str, List[Any]]:
        """Get the variants as a dictionary.

        Returns:
            Dictionary mapping variant keys to lists of values.
            For example: {'python': ['3.9', '3.10'], 'numpy': ['1.19']}
        """
        return self._inner.variants

    @property
    def zip_keys(self) -> Optional[List[List[str]]]:
        """Get the zip keys if defined.

        Zip keys are used to "zip" together variants to create specific combinations
        instead of the full Cartesian product.

        Returns:
            List of lists of variant keys to zip together, or None.
            For example: [['python', 'numpy'], ['openssl', 'libcurl']]
        """
        return self._inner.zip_keys

    def __repr__(self) -> str:
        """Return string representation."""
        return repr(self._inner)


class BuildConfiguration:
    """Build configuration for an output.

    This contains all the resolved configuration for building a specific output,
    including platforms, channels, variant selections, and computed hash.

    Example:
        >>> output = parse_recipe_with_variants("recipe.yaml")[0]
        >>> config = output.build_configuration
        >>> print(config.target_platform)
        'linux-64'
        >>> print(config.variant)
        {'python': '3.11', 'numpy': '1.19'}
    """

    def __init__(self, inner: PyBuildConfiguration):
        """Initialize from PyBuildConfiguration (internal use)."""
        self._inner = inner

    @property
    def target_platform(self) -> str:
        """Get the target platform.

        Returns:
            The target platform string (e.g., 'linux-64').
        """
        return self._inner.target_platform

    @property
    def host_platform(self) -> str:
        """Get the host platform.

        Returns:
            The host platform string.
        """
        return self._inner.host_platform

    @property
    def build_platform(self) -> str:
        """Get the build platform.

        Returns:
            The build platform string.
        """
        return self._inner.build_platform

    @property
    def variant(self) -> Dict[str, Any]:
        """Get the variant configuration used for this build.

        Returns:
            Dictionary of resolved variant variables.
        """
        return self._inner.variant

    @property
    def channels(self) -> List[str]:
        """Get the list of channels.

        Returns:
            List of channel URLs.
        """
        return self._inner.channels

    @property
    def hash(self) -> str:
        """Get the computed build hash.

        The hash is computed from the variant configuration and is used
        in the build string.

        Returns:
            The build hash string.
        """
        return self._inner.hash

    def __repr__(self) -> str:
        """Return string representation."""
        return repr(self._inner)


class Output:
    """Represents a single package output from a recipe.

    An output is a specific build of a package with resolved variant configuration.
    A single recipe can produce multiple outputs based on variant combinations.

    Example:
        >>> outputs = parse_recipe_with_variants("recipe.yaml")
        >>> for output in outputs:
        ...     print(f"{output.name}-{output.version}-{output.build_string}")
        ...     print(f"  Platform: {output.build_configuration.target_platform}")
        ...     print(f"  Variant: {output.build_configuration.variant}")
    """

    def __init__(self, inner: PyOutput):
        """Initialize from PyOutput (internal use)."""
        self._inner = inner

    @property
    def name(self) -> str:
        """Get the package name.

        Returns:
            The normalized package name.
        """
        return self._inner.name

    @property
    def version(self) -> str:
        """Get the package version.

        Returns:
            The version string.
        """
        return self._inner.version

    @property
    def build_string(self) -> str:
        """Get the build string.

        The build string typically includes variant information and the hash.

        Returns:
            The build string.
        """
        return self._inner.build_string

    @property
    def identifier(self) -> str:
        """Get the full package identifier.

        Returns:
            The full identifier in the format: name-version-build_string
        """
        return self._inner.identifier

    @property
    def build_configuration(self) -> BuildConfiguration:
        """Get the build configuration for this output.

        Returns:
            The BuildConfiguration object.
        """
        return BuildConfiguration(self._inner.build_configuration)

    @property
    def recipe(self) -> Dict[str, Any]:
        """Get the full recipe as a dictionary.

        Returns:
            The complete rendered recipe as a Python dictionary.
        """
        return self._inner.recipe

    def build(self, tool_config: Optional["ToolConfiguration"] = None) -> Path:
        """Build this output.

        Args:
            tool_config: Optional tool configuration. If not provided,
                        an error will be raised.

        Returns:
            Path to the built package artifact.

        Raises:
            RattlerBuildError: If the build fails.

        Example:
            >>> output = parse_recipe_with_variants("recipe.yaml")[0]
            >>> # artifact_path = output.build(tool_config)
        """
        py_tool_config = tool_config._inner if tool_config else None
        return self._inner.build(py_tool_config)

    def __repr__(self) -> str:
        """Return string representation."""
        return repr(self._inner)


class ToolConfiguration:
    """Configuration for the build tool.

    This wraps the internal tool configuration that controls various aspects
    of the build process.
    """

    def __init__(self, inner: PyToolConfiguration):
        """Initialize from PyToolConfiguration (internal use)."""
        self._inner = inner

    def __repr__(self) -> str:
        """Return string representation."""
        return repr(self._inner)


def parse_recipe_with_variants(
    recipe_path: Union[str, Path],
    build_platform: Optional[str] = None,
    target_platform: Optional[str] = None,
    host_platform: Optional[str] = None,
    channels: Optional[List[str]] = None,
    output_dir: Optional[Union[str, Path]] = None,
) -> List[Output]:
    """Parse a recipe YAML file and return all variant outputs.

    This function parses a recipe file, expands all variants, and returns a list
    of Output objects representing each build configuration.

    Args:
        recipe_path: Path to the recipe.yaml file.
        build_platform: The build platform (defaults to current platform).
        target_platform: The target platform (defaults to current platform).
        host_platform: The host platform (defaults to current platform).
        channels: List of channel URLs to use for dependency resolution.
        output_dir: Directory for build outputs.

    Returns:
        List of Output objects, one for each variant combination.

    Raises:
        RattlerBuildError: If recipe parsing or variant expansion fails.

    Example:
        >>> outputs = parse_recipe_with_variants(
        ...     "recipe.yaml",
        ...     target_platform="linux-64",
        ...     channels=["conda-forge", "bioconda"]
        ... )
        >>> for output in outputs:
        ...     print(f"{output.identifier}")
        ...     variant = output.build_configuration.variant
        ...     print(f"  Python: {variant.get('python', 'N/A')}")
    """
    recipe_path_obj = Path(recipe_path)
    output_dir_obj = Path(output_dir) if output_dir else None

    py_outputs = _parse_recipe_with_variants(
        recipe_path=recipe_path_obj,
        build_platform=build_platform,
        target_platform=target_platform,
        host_platform=host_platform,
        channels=channels,
        output_dir=output_dir_obj,
    )

    return [Output(py_output) for py_output in py_outputs]


__all__ = [
    "SelectorConfig",
    "VariantConfig",
    "BuildConfiguration",
    "Output",
    "ToolConfiguration",
    "parse_recipe_with_variants",
]
