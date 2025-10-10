"""Object-oriented interface for variant configuration.

This module provides Python wrappers around the Rust implementation of variant
configuration handling in rattler-build. Variant configurations allow you to
define build matrices for packages.

Examples:
    Create a simple variant configuration:

    >>> from rattler_build import Pin, VariantConfig
    >>> pin = Pin(max_pin="x.x", min_pin="x.x.x")
    >>> config = VariantConfig(
    ...     variants={
    ...         "python": ["3.9", "3.10", "3.11"],
    ...         "numpy": ["1.21", "1.22"]
    ...     }
    ... )
    >>> len(config.variants["python"])
    3

    Using zip_keys to create specific variant combinations:

    >>> config = VariantConfig(
    ...     zip_keys=[["python", "numpy"]],
    ...     variants={
    ...         "python": ["3.9", "3.10", "3.11"],
    ...         "numpy": ["1.21", "1.22", "1.23"]
    ...     }
    ... )
    >>> config.zip_keys
    [['python', 'numpy']]

    Adding pin_run_as_build constraints:

    >>> config = VariantConfig(
    ...     pin_run_as_build={
    ...         "python": Pin(max_pin="x.x"),
    ...         "numpy": Pin(max_pin="x.x", min_pin="x.x.x.x")
    ...     },
    ...     variants={
    ...         "python": ["3.9", "3.10"],
    ...         "numpy": ["1.21"]
    ...     }
    ... )
    >>> "python" in config.pin_run_as_build
    True
"""

from typing import Dict, List, Optional, Any, Union
from pathlib import Path
from .rattler_build import PyPin, PyVariantConfig
from .recipe import SelectorConfig


class Pin:
    """Pin configuration for a package version constraint.

    A Pin defines version constraints for packages using a pin syntax
    (e.g., "x.x" means pin to the major.minor version).

    Args:
        max_pin: Maximum version pin pattern (e.g., "x.x" for major.minor)
        min_pin: Minimum version pin pattern (e.g., "x.x.x" for major.minor.patch)

    Examples:
        >>> pin = Pin(max_pin="x.x")
        >>> pin.max_pin
        'x.x'

        >>> pin = Pin(max_pin="x.x", min_pin="x.x.x.x")
        >>> pin.min_pin
        'x.x.x.x'

        >>> pin = Pin()
        >>> pin.max_pin is None
        True

        Modify pin after creation:

        >>> pin = Pin(max_pin="x.x")
        >>> pin.min_pin = "x.x.x"
        >>> pin.min_pin
        'x.x.x'
    """

    def __init__(self, max_pin: Optional[str] = None, min_pin: Optional[str] = None):
        """Initialize a Pin with optional max and min pin patterns."""
        self._inner = PyPin(max_pin=max_pin, min_pin=min_pin)

    @property
    def max_pin(self) -> Optional[str]:
        """Get the maximum pin pattern.

        Returns:
            The maximum pin pattern or None if not set.

        Examples:
            >>> pin = Pin(max_pin="x.x")
            >>> pin.max_pin
            'x.x'
        """
        return self._inner.max_pin

    @max_pin.setter
    def max_pin(self, value: Optional[str]) -> None:
        """Set the maximum pin pattern.

        Args:
            value: The maximum pin pattern or None.

        Examples:
            >>> pin = Pin()
            >>> pin.max_pin = "x.x.x"
            >>> pin.max_pin
            'x.x.x'
        """
        self._inner.max_pin = value

    @property
    def min_pin(self) -> Optional[str]:
        """Get the minimum pin pattern.

        Returns:
            The minimum pin pattern or None if not set.

        Examples:
            >>> pin = Pin(min_pin="x.x.x.x")
            >>> pin.min_pin
            'x.x.x.x'
        """
        return self._inner.min_pin

    @min_pin.setter
    def min_pin(self, value: Optional[str]) -> None:
        """Set the minimum pin pattern.

        Args:
            value: The minimum pin pattern or None.

        Examples:
            >>> pin = Pin()
            >>> pin.min_pin = "x.x"
            >>> pin.min_pin
            'x.x'
        """
        self._inner.min_pin = value

    def __repr__(self) -> str:
        """Return a string representation of the Pin.

        Examples:
            >>> pin = Pin(max_pin="x.x", min_pin="x.x.x")
            >>> repr(pin)
            "Pin(max_pin='x.x', min_pin='x.x.x')"
        """
        return f"Pin(max_pin={self.max_pin!r}, min_pin={self.min_pin!r})"

    def __eq__(self, other: object) -> bool:
        """Check equality with another Pin.

        Examples:
            >>> pin1 = Pin(max_pin="x.x")
            >>> pin2 = Pin(max_pin="x.x")
            >>> pin1 == pin2
            True
            >>> pin3 = Pin(max_pin="x.x.x")
            >>> pin1 == pin3
            False
        """
        if not isinstance(other, Pin):
            return NotImplemented
        return self.max_pin == other.max_pin and self.min_pin == other.min_pin


class VariantConfig:
    """Variant configuration for package builds.

    A VariantConfig defines the build matrix for a package, including:
    - Variable variants (e.g., different Python versions)
    - Pin configurations for run-time dependencies
    - Zip keys to create specific variant combinations

    Args:
        pin_run_as_build: Mapping of package names to Pin configurations
        zip_keys: List of lists defining which variants should be zipped together
        variants: Mapping of variant names to lists of possible values

    Examples:
        Create a basic variant configuration:

        >>> config = VariantConfig(
        ...     variants={
        ...         "python": ["3.9", "3.10", "3.11"]
        ...     }
        ... )
        >>> config.variants["python"]
        ['3.9', '3.10', '3.11']

        Use pin_run_as_build:

        >>> config = VariantConfig(
        ...     pin_run_as_build={
        ...         "python": Pin(max_pin="x.x")
        ...     },
        ...     variants={
        ...         "python": ["3.9", "3.10"]
        ...     }
        ... )
        >>> config.pin_run_as_build["python"].max_pin
        'x.x'

        Use zip_keys to control variant combinations:

        >>> config = VariantConfig(
        ...     zip_keys=[["python", "numpy"]],
        ...     variants={
        ...         "python": ["3.9", "3.10"],
        ...         "numpy": ["1.21", "1.22"]
        ...     }
        ... )
        >>> config.zip_keys
        [['python', 'numpy']]

        Modify configuration after creation:

        >>> config = VariantConfig()
        >>> config.variants = {"cuda": ["11.8", "12.0"]}
        >>> config.variants["cuda"]
        ['11.8', '12.0']
    """

    def __init__(
        self,
        pin_run_as_build: Optional[Dict[str, Pin]] = None,
        zip_keys: Optional[List[List[str]]] = None,
        variants: Optional[Dict[str, List[Any]]] = None,
    ):
        """Initialize a VariantConfig with optional parameters."""
        # Convert Pin wrappers to PyPin
        py_pin_run_as_build = None
        if pin_run_as_build is not None:
            py_pin_run_as_build = {k: v._inner for k, v in pin_run_as_build.items()}

        self._inner = PyVariantConfig(
            pin_run_as_build=py_pin_run_as_build,
            zip_keys=zip_keys,
            variants=variants,
        )

    @property
    def pin_run_as_build(self) -> Optional[Dict[str, Pin]]:
        """Get the pin_run_as_build mapping.

        Returns:
            Dictionary mapping package names to Pin objects, or None.

        Examples:
            >>> config = VariantConfig(
            ...     pin_run_as_build={"python": Pin(max_pin="x.x")}
            ... )
            >>> config.pin_run_as_build["python"].max_pin
            'x.x'
        """
        inner_pins = self._inner.pin_run_as_build
        if inner_pins is None:
            return None
        return {k: Pin(max_pin=v.max_pin, min_pin=v.min_pin) for k, v in inner_pins.items()}

    @pin_run_as_build.setter
    def pin_run_as_build(self, value: Optional[Dict[str, Pin]]) -> None:
        """Set the pin_run_as_build mapping.

        Args:
            value: Dictionary mapping package names to Pin objects, or None.

        Examples:
            >>> config = VariantConfig()
            >>> config.pin_run_as_build = {"numpy": Pin(max_pin="x.x")}
            >>> config.pin_run_as_build["numpy"].max_pin
            'x.x'
        """
        if value is None:
            self._inner.pin_run_as_build = None
        else:
            self._inner.pin_run_as_build = {k: v._inner for k, v in value.items()}

    @property
    def zip_keys(self) -> Optional[List[List[str]]]:
        """Get the zip_keys configuration.

        Zip keys are used to "zip" together variants to create specific
        combinations. For example, if you have python=[3.9, 3.10] and
        numpy=[1.21, 1.22], and zip_keys=[["python", "numpy"]], then
        the variants will be (3.9, 1.21) and (3.10, 1.22) instead of
        all four combinations.

        Returns:
            List of lists of variant names to zip together, or None.

        Examples:
            >>> config = VariantConfig(zip_keys=[["python", "numpy"]])
            >>> config.zip_keys
            [['python', 'numpy']]
        """
        return self._inner.zip_keys

    @zip_keys.setter
    def zip_keys(self, value: Optional[List[List[str]]]) -> None:
        """Set the zip_keys configuration.

        Args:
            value: List of lists of variant names to zip together, or None.

        Examples:
            >>> config = VariantConfig()
            >>> config.zip_keys = [["cuda", "cudnn"]]
            >>> config.zip_keys
            [['cuda', 'cudnn']]
        """
        self._inner.zip_keys = value

    @property
    def variants(self) -> Dict[str, List[Any]]:
        """Get the variants mapping.

        Returns:
            Dictionary mapping variant names to lists of possible values.

        Examples:
            >>> config = VariantConfig(variants={"python": ["3.9", "3.10"]})
            >>> config.variants["python"]
            ['3.9', '3.10']

            Variants can contain different types:

            >>> config = VariantConfig(variants={
            ...     "python": ["3.9", "3.10"],
            ...     "cuda_enabled": [True, False],
            ...     "cuda_version": [11, 12]
            ... })
            >>> config.variants["cuda_enabled"]
            [True, False]
        """
        return self._inner.variants

    @variants.setter
    def variants(self, value: Dict[str, List[Any]]) -> None:
        """Set the variants mapping.

        Args:
            value: Dictionary mapping variant names to lists of possible values.

        Examples:
            >>> config = VariantConfig()
            >>> config.variants = {"rust": ["1.70", "1.71"]}
            >>> config.variants["rust"]
            ['1.70', '1.71']
        """
        self._inner.variants = value

    def __repr__(self) -> str:
        """Return a string representation of the VariantConfig.

        Examples:
            >>> config = VariantConfig(variants={"python": ["3.9"]})
            >>> "VariantConfig" in repr(config)
            True
        """
        pin_keys = list(self.pin_run_as_build.keys()) if self.pin_run_as_build else []
        variant_keys = list(self.variants.keys())
        return (
            f"VariantConfig(" f"pin_run_as_build={pin_keys}, " f"zip_keys={self.zip_keys}, " f"variants={variant_keys})"
        )

    def __eq__(self, other: object) -> bool:
        """Check equality with another VariantConfig.

        Examples:
            >>> config1 = VariantConfig(variants={"python": ["3.9"]})
            >>> config2 = VariantConfig(variants={"python": ["3.9"]})
            >>> config1 == config2
            True
        """
        if not isinstance(other, VariantConfig):
            return NotImplemented
        return (
            self.pin_run_as_build == other.pin_run_as_build
            and self.zip_keys == other.zip_keys
            and self.variants == other.variants
        )

    def merge(self, other: "VariantConfig") -> None:
        """Merge another VariantConfig into this one.

        This modifies the current config in-place by merging values from `other`:
        - Variants are extended (keys from `other` replace keys in `self`)
        - pin_run_as_build entries are extended
        - zip_keys are replaced (not merged)

        Args:
            other: Another VariantConfig to merge into this one

        Examples:
            >>> config1 = VariantConfig(variants={"python": ["3.9"]})
            >>> config2 = VariantConfig(variants={"numpy": ["1.21"]})
            >>> config1.merge(config2)
            >>> sorted(config1.variants.keys())
            ['numpy', 'python']

            Merging replaces existing keys:

            >>> config1 = VariantConfig(variants={"python": ["3.9"]})
            >>> config2 = VariantConfig(variants={"python": ["3.10"]})
            >>> config1.merge(config2)
            >>> config1.variants["python"]
            ['3.10']

            zip_keys are replaced, not merged:

            >>> config1 = VariantConfig(
            ...     zip_keys=[["python", "numpy"]],
            ...     variants={"python": ["3.9"]}
            ... )
            >>> config2 = VariantConfig(
            ...     zip_keys=[["cuda", "cudnn"]],
            ...     variants={"cuda": ["11.8"]}
            ... )
            >>> config1.merge(config2)
            >>> config1.zip_keys
            [['cuda', 'cudnn']]
        """
        self._inner.merge(other._inner)

    @staticmethod
    def from_file(file: Union[str, Path], selector_config: Optional[SelectorConfig] = None) -> "VariantConfig":
        """Load a VariantConfig from a single YAML file.

        This function loads a single variant configuration file. The file can be
        either a variant config file (e.g., variants.yaml) or a conda-build config
        file (conda_build_config.yaml).

        Note: The target_platform and build_platform are automatically inserted
        into the variants based on the selector_config.

        Args:
            file: Path to variant configuration file
            selector_config: Optional SelectorConfig for platform selection and rendering.
                           If not provided, uses current platform defaults.

        Returns:
            A new VariantConfig with the configuration from the file.

        Raises:
            RattlerBuildError: If file cannot be loaded or parsed

        Examples:
            Load a single variant config file:

            >>> # config = VariantConfig.from_file("variants.yaml")
            >>> # config.variants["python"]
            >>> # ['3.9', '3.10', '3.11']

            Load with specific platform:

            >>> from rattler_build import SelectorConfig
            >>> selector = SelectorConfig(target_platform="linux-64")
            >>> # config = VariantConfig.from_file(
            >>> #     "variants.yaml",
            >>> #     selector_config=selector
            >>> # )
        """
        # Convert string path to Path object
        path = Path(file) if isinstance(file, str) else file

        # Create default selector config if not provided
        if selector_config is None:
            selector_config = SelectorConfig()

        # Load from Rust
        rust_config = PyVariantConfig.from_file(path, selector_config._config)

        # Wrap in Python class
        result = VariantConfig()
        result._inner = rust_config
        return result

    @staticmethod
    def from_files(files: List[Union[str, Path]], selector_config: Optional[SelectorConfig] = None) -> "VariantConfig":
        """Load a VariantConfig from one or more YAML files.

        This function loads and merges multiple variant configuration files.
        Files can be either:
        - Variant config files (e.g., variants.yaml)
        - Conda-build config files (conda_build_config.yaml)

        Files are processed in order, with later files overriding earlier ones
        for the same keys (values are not merged, only replaced).

        Args:
            files: List of paths to variant configuration files
            selector_config: Optional SelectorConfig for platform selection and rendering.
                           If not provided, uses current platform defaults.

        Returns:
            A new VariantConfig with the merged configuration from all files.

        Raises:
            RattlerBuildError: If files cannot be loaded or parsed

        Examples:
            Load a single variant config file:

            >>> # Assuming variants.yaml exists with python versions
            >>> # config = VariantConfig.from_files(["variants.yaml"])
            >>> # config.variants["python"]
            >>> # ['3.9', '3.10', '3.11']

            Load and merge multiple config files:

            >>> # config = VariantConfig.from_files([
            >>> #     "variants.yaml",
            >>> #     "conda_build_config.yaml"
            >>> # ])

            Load with specific platform:

            >>> from rattler_build import SelectorConfig
            >>> selector = SelectorConfig(target_platform="linux-64")
            >>> # config = VariantConfig.from_files(
            >>> #     ["variants.yaml"],
            >>> #     selector_config=selector
            >>> # )
        """
        # Convert string paths to Path objects
        path_list = [Path(f) if isinstance(f, str) else f for f in files]

        # Create default selector config if not provided
        if selector_config is None:
            selector_config = SelectorConfig()

        # Load from Rust
        rust_config = PyVariantConfig.from_files(path_list, selector_config._config)

        # Wrap in Python class
        result = VariantConfig()
        result._inner = rust_config
        return result


__all__ = ["Pin", "VariantConfig"]
