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

    This class provides a dict-like interface for managing variants.

    Example:
        >>> # Create from dict
        >>> config = VariantConfig({
        ...     "python": ["3.8", "3.9", "3.10"],
        ...     "numpy": ["1.21", "1.22"]
        ... })
        >>> len(config.combinations())  # 3 * 2 = 6 combinations
        6

        >>> # Dict-like access
        >>> config["python"] = ["3.9", "3.10"]
        >>> print(config["python"])
        ['3.9', '3.10']

        >>> # Traditional method calls
        >>> config.set_values("compiler", ["gcc", "clang"])
        >>> print(config.get_values("compiler"))
        ['gcc', 'clang']

        >>> # Load from YAML file
        >>> config = VariantConfig.from_file("variant_config.yaml")
        >>> print(config.keys())
    """

    def __init__(self, variants: Optional[Union[Dict[str, List[Any]], _VariantConfig]] = None):
        """
        Create a new VariantConfig.

        Args:
            variants: Either a dictionary mapping variant keys to value lists,
                     or an existing _VariantConfig instance. If None, creates empty config.

        Example:
            >>> # Create from dict
            >>> config = VariantConfig({"python": ["3.9", "3.10"]})

            >>> # Create empty
            >>> config = VariantConfig()
        """
        if variants is None:
            self._inner = _VariantConfig()
        elif isinstance(variants, dict):
            self._inner = _VariantConfig()
            for key, values in variants.items():
                self._inner.set_values(key, values)
        else:
            self._inner = variants

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

    def __getitem__(self, key: str) -> List[Any]:
        """
        Get values for a variant key using dict-like access.

        Args:
            key: The variant key name

        Returns:
            List of values for the key

        Raises:
            KeyError: If the key doesn't exist

        Example:
            >>> config = VariantConfig({"python": ["3.9", "3.10"]})
            >>> config["python"]
            ['3.9', '3.10']
        """
        values = self._inner.get_values(key)
        if values is None:
            raise KeyError(f"Variant key '{key}' not found")
        return values

    def __setitem__(self, key: str, values: List[Any]) -> None:
        """
        Set values for a variant key using dict-like access.

        Args:
            key: The variant key name
            values: List of values for this key

        Example:
            >>> config = VariantConfig()
            >>> config["python"] = ["3.9", "3.10"]
            >>> config["numpy"] = ["1.21", "1.22"]
        """
        self._inner.set_values(key, values)

    def __contains__(self, key: str) -> bool:
        """
        Check if a variant key exists.

        Args:
            key: The variant key name

        Returns:
            True if the key exists, False otherwise

        Example:
            >>> config = VariantConfig({"python": ["3.9"]})
            >>> "python" in config
            True
            >>> "ruby" in config
            False
        """
        return self._inner.get_values(key) is not None

    def __delitem__(self, key: str) -> None:
        """
        Delete a variant key (not implemented - raises NotImplementedError).

        Args:
            key: The variant key name

        Raises:
            NotImplementedError: Deletion is not supported
        """
        raise NotImplementedError("Deletion of variant keys is not supported")

    def __iter__(self):
        """
        Iterate over variant keys.

        Returns:
            Iterator over variant key names

        Example:
            >>> config = VariantConfig({"python": ["3.9"], "numpy": ["1.21"]})
            >>> list(config)
            ['numpy', 'python']
        """
        return iter(self.keys())

    def items(self):
        """
        Get all variant key-value pairs.

        Returns:
            Iterator of (key, values) tuples

        Example:
            >>> config = VariantConfig({"python": ["3.9", "3.10"]})
            >>> dict(config.items())
            {'python': ['3.9', '3.10']}
        """
        return self.to_dict().items()

    def values(self):
        """
        Get all variant value lists.

        Returns:
            Iterator of value lists

        Example:
            >>> config = VariantConfig({"python": ["3.9", "3.10"]})
            >>> list(config.values())
            [['3.9', '3.10']]
        """
        return self.to_dict().values()

    def get(self, key: str, default: Optional[List[Any]] = None) -> Optional[List[Any]]:
        """
        Get values for a variant key with a default.

        Args:
            key: The variant key name
            default: Default value if key doesn't exist

        Returns:
            List of values for the key, or default if key doesn't exist

        Example:
            >>> config = VariantConfig({"python": ["3.9"]})
            >>> config.get("python")
            ['3.9']
            >>> config.get("ruby", ["2.7"])
            ['2.7']
        """
        values = self._inner.get_values(key)
        return values if values is not None else default

    def update(self, other: Union["VariantConfig", Dict[str, List[Any]]]) -> None:
        """
        Update this config with values from another config or dict.

        Args:
            other: Another VariantConfig or dict to merge

        Example:
            >>> config = VariantConfig({"python": ["3.9"]})
            >>> config.update({"numpy": ["1.21"]})
            >>> config.keys()
            ['numpy', 'python']
        """
        if isinstance(other, VariantConfig):
            self.merge(other)
        elif isinstance(other, dict):
            for key, values in other.items():
                self.set_values(key, values)
        else:
            raise TypeError(f"Expected VariantConfig or dict, got {type(other)}")

    def __repr__(self) -> str:
        return repr(self._inner)

    def __str__(self) -> str:
        return f"VariantConfig(keys={self.keys()})"
