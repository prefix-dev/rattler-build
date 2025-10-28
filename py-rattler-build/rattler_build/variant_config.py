"""
VariantConfig - Manage variant configuration for recipe builds.

This module provides Python bindings for rattler-build's VariantConfig,
which manages variant matrices for building packages with different configurations.
"""

from pathlib import Path
from typing import Any, Dict, List, Optional, Union
from .rattler_build import VariantConfig as _VariantConfig, PyJinjaConfig
from .jinja_config import JinjaConfig

__all__ = ["VariantConfig"]


class VariantConfig:
    """
    Configuration for build variants.

    Variants allow building the same recipe with different configurations,
    such as different Python versions, compilers, or other parameters.

    Example:
        >>> config = VariantConfig()
        >>> config.set_values("python", ["3.8", "3.9", "3.10"])
        >>> config.set_values("numpy", ["1.21", "1.22"])
        >>> combinations = config.combinations()
        >>> len(combinations)  # 3 * 2 = 6 combinations
        6

        >>> # Load from YAML file
        >>> config = VariantConfig.from_file("variant_config.yaml")
        >>> print(config.keys())
    """

    def __init__(self, inner: Optional[_VariantConfig] = None):
        self._inner = inner if inner is not None else _VariantConfig()

    @classmethod
    def from_file(cls, path: Union[str, Path]) -> "VariantConfig":
        """
        Load VariantConfig from a YAML file (variants.yaml format).

        Args:
            path: Path to the variant configuration YAML file

        Returns:
            A new VariantConfig instance

        Example:
            >>> config = VariantConfig.from_file("variants.yaml")
        """
        return cls(_VariantConfig.from_file(Path(path)))

    @classmethod
    def from_file_with_context(
        cls, path: Union[str, Path], jinja_config: Union[PyJinjaConfig, JinjaConfig]
    ) -> "VariantConfig":
        """
        Load VariantConfig from a YAML file with a JinjaConfig context (variants.yaml format).

        This allows evaluation of conditionals and templates in the variant file.
        The jinja_config provides platform information and other context needed for evaluation.

        Args:
            path: Path to the variant configuration YAML file
            jinja_config: JinjaConfig providing context for evaluation

        Returns:
            A new VariantConfig instance

        Example:
            >>> from rattler_build import JinjaConfig
            >>> jinja_config = JinjaConfig(target_platform="linux-64")
            >>> config = VariantConfig.from_file_with_context("variants.yaml", jinja_config)
        """
        # Convert JinjaConfig to PyJinjaConfig if needed
        py_config = jinja_config._config if isinstance(jinja_config, JinjaConfig) else jinja_config
        return cls(_VariantConfig.from_file_with_context(Path(path), py_config))

    @classmethod
    def from_conda_build_config(
        cls, path: Union[str, Path], jinja_config: Union[PyJinjaConfig, JinjaConfig]
    ) -> "VariantConfig":
        """
        Load VariantConfig from a conda_build_config.yaml file.

        This supports the legacy conda-build format with `# [selector]` syntax.
        Selectors are evaluated using the provided JinjaConfig.

        Args:
            path: Path to the conda_build_config.yaml file
            jinja_config: JinjaConfig providing context for selector evaluation

        Returns:
            A new VariantConfig instance

        Example:
            >>> from rattler_build import JinjaConfig
            >>> jinja_config = JinjaConfig(target_platform="linux-64")
            >>> config = VariantConfig.from_conda_build_config("conda_build_config.yaml", jinja_config)
        """
        # Convert JinjaConfig to PyJinjaConfig if needed
        py_config = jinja_config._config if isinstance(jinja_config, JinjaConfig) else jinja_config
        return cls(_VariantConfig.from_conda_build_config(Path(path), py_config))

    @classmethod
    def from_yaml(cls, yaml: str) -> "VariantConfig":
        """
        Load VariantConfig from a YAML string (variants.yaml format).

        Args:
            yaml: YAML string containing variant configuration

        Returns:
            A new VariantConfig instance

        Example:
            >>> yaml_str = '''
            ... python:
            ...   - "3.8"
            ...   - "3.9"
            ... '''
            >>> config = VariantConfig.from_yaml(yaml_str)
        """
        return cls(_VariantConfig.from_yaml(yaml))

    @classmethod
    def from_yaml_with_context(cls, yaml: str, jinja_config: Union[PyJinjaConfig, JinjaConfig]) -> "VariantConfig":
        """
        Load VariantConfig from a YAML string with a JinjaConfig context (variants.yaml format).

        This allows evaluation of conditionals and templates in the variant YAML.
        The jinja_config provides platform information and other context needed for evaluation.

        Args:
            yaml: YAML string containing variant configuration
            jinja_config: JinjaConfig providing context for evaluation

        Returns:
            A new VariantConfig instance

        Example:
            >>> from rattler_build import JinjaConfig
            >>> yaml_str = '''
            ... c_compiler:
            ...   - if: unix
            ...     then: gcc
            ...   - if: win
            ...     then: msvc
            ... '''
            >>> jinja_config = JinjaConfig(target_platform="linux-64")
            >>> config = VariantConfig.from_yaml_with_context(yaml_str, jinja_config)
        """
        # Convert JinjaConfig to PyJinjaConfig if needed
        py_config = jinja_config._config if isinstance(jinja_config, JinjaConfig) else jinja_config
        return cls(_VariantConfig.from_yaml_with_context(yaml, py_config))

    def keys(self) -> List[str]:
        """
        Get all variant keys.

        Returns:
            List of variant key names

        Example:
            >>> config = VariantConfig()
            >>> config.set_values("python", ["3.8", "3.9"])
            >>> config.set_values("numpy", ["1.21"])
            >>> config.keys()
            ['numpy', 'python']
        """
        return self._inner.keys()

    @property
    def zip_keys(self) -> Optional[List[List[str]]]:
        """
        Get zip_keys - groups of keys that should be zipped together.

        Zip keys ensure that certain variant keys are synchronized when creating
        combinations. For example, if python and numpy are zipped, then
        python=3.9 will always be paired with numpy=1.20, not with other numpy versions.

        Returns:
            List of groups (each group is a list of keys), or None if no zip keys are defined

        Example:
            >>> config = VariantConfig()
            >>> config.set_values("python", ["3.9", "3.10"])
            >>> config.set_values("numpy", ["1.20", "1.21"])
            >>> config.zip_keys = [["python", "numpy"]]
            >>> len(config.combinations())  # 2, not 4
            2
        """
        return self._inner.zip_keys

    @zip_keys.setter
    def zip_keys(self, value: Optional[List[List[str]]]) -> None:
        """
        Set zip_keys - groups of keys that should be zipped together.

        Args:
            value: List of groups (each group is a list of keys), or None to clear

        Example:
            >>> config = VariantConfig()
            >>> config.zip_keys = [["python", "numpy"], ["c_compiler", "cxx_compiler"]]
        """
        self._inner.zip_keys = value

    def get_values(self, key: str) -> Optional[List[Any]]:
        """
        Get values for a specific variant key.

        Args:
            key: The variant key name

        Returns:
            List of values for the key, or None if key doesn't exist

        Example:
            >>> config = VariantConfig()
            >>> config.set_values("python", ["3.8", "3.9", "3.10"])
            >>> config.get_values("python")
            ['3.8', '3.9', '3.10']
        """
        return self._inner.get_values(key)

    def set_values(self, key: str, values: List[Any]) -> None:
        """
        Set values for a variant key.

        Args:
            key: The variant key name
            values: List of values for this key

        Example:
            >>> config = VariantConfig()
            >>> config.set_values("python", ["3.8", "3.9", "3.10"])
            >>> config.set_values("numpy", ["1.21", "1.22"])
        """
        self._inner.set_values(key, values)

    def to_dict(self) -> Dict[str, List[Any]]:
        """
        Get all variants as a dictionary.

        Returns:
            Dictionary mapping variant keys to their value lists

        Example:
            >>> config = VariantConfig()
            >>> config.set_values("python", ["3.8", "3.9"])
            >>> config.to_dict()
            {'python': ['3.8', '3.9']}
        """
        return self._inner.to_dict()

    def merge(self, other: "VariantConfig") -> None:
        """
        Merge another VariantConfig into this one.

        Args:
            other: Another VariantConfig to merge

        Example:
            >>> config1 = VariantConfig()
            >>> config1.set_values("python", ["3.8", "3.9"])
            >>> config2 = VariantConfig()
            >>> config2.set_values("numpy", ["1.21"])
            >>> config1.merge(config2)
            >>> config1.keys()
            ['numpy', 'python']
        """
        self._inner.merge(other._inner)

    def combinations(self) -> List[Dict[str, Any]]:
        """
        Generate all combinations of variant values.

        Returns:
            List of dictionaries, each representing one variant combination

        Example:
            >>> config = VariantConfig()
            >>> config.set_values("python", ["3.8", "3.9"])
            >>> config.set_values("numpy", ["1.21", "1.22"])
            >>> combos = config.combinations()
            >>> len(combos)
            4
            >>> combos[0]
            {'python': '3.8', 'numpy': '1.21'}
        """
        return self._inner.combinations()

    def __len__(self) -> int:
        """Get the number of variant keys."""
        return len(self._inner)

    def __repr__(self) -> str:
        return repr(self._inner)

    def __str__(self) -> str:
        return f"VariantConfig(keys={self.keys()})"
