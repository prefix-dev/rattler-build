"""
VariantConfig - Manage variant configuration for recipe builds.

This module provides Python bindings for rattler-build's VariantConfig,
which manages variant matrices for building packages with different configurations.
"""

from collections.abc import ItemsView, Iterator, ValuesView
from pathlib import Path
from typing import Any

from rattler_build._rattler_build import VariantConfig as _VariantConfig
from rattler_build.jinja_config import JinjaConfig


class VariantConfig:
    """
    Configuration for build variants.

    Variants allow building the same recipe with different configurations,
    such as different Python versions, compilers, or other parameters.

    This class provides a dict-like interface for managing variants.

    Example:
        ```python
        # Create from dict
        config = VariantConfig({
            "python": ["3.8", "3.9", "3.10"],
            "numpy": ["1.21", "1.22"]
        })
        len(config.combinations())  # 3 * 2 = 6 combinations
        # 6

        # Dict-like access
        print(config["python"])
        # ['3.8', '3.9', '3.10']

        # Get values
        print(config.get_values("python"))
        # ['3.8', '3.9', '3.10']

        # Load from YAML file
        config = VariantConfig.from_file("variant_config.yaml")
        print(config.keys())
        ```
    """

    def __init__(
        self,
        variants: dict[str, list[Any]] | None = None,
        zip_keys: list[list[str]] | None = None,
    ):
        """
        Create a new VariantConfig.

        Args:
            variants: A dictionary mapping variant keys to value lists.
                     If None, creates empty config.
            zip_keys: Optional list of groups (each group is a list of keys) that should be
                     zipped together. Ensures that certain variant keys are synchronized.

        Example:
            ```python
            # Create from dict
            config = VariantConfig({"python": ["3.9", "3.10"]})

            # Create with zip_keys
            config = VariantConfig(
                {"python": ["3.9", "3.10"], "numpy": ["1.21", "1.22"]},
                zip_keys=[["python", "numpy"]]
            )

            # Create empty
            config = VariantConfig()
            ```
        """

        self._inner = _VariantConfig(variants=variants, zip_keys=zip_keys)

    @classmethod
    def from_file(cls, path: str | Path) -> "VariantConfig":
        """
        Load VariantConfig from a YAML file (variants.yaml format).

        Args:
            path: Path to the variant configuration YAML file

        Returns:
            A new VariantConfig instance

        Example:
            ```python
            config = VariantConfig.from_file("variants.yaml")
            ```
        """
        variant_config = cls.__new__(cls)
        variant_config._inner = _VariantConfig.from_file(Path(path))
        return variant_config

    @classmethod
    def from_file_with_context(cls, path: str | Path, jinja_config: JinjaConfig) -> "VariantConfig":
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
            ```python
            from rattler_build import JinjaConfig

            jinja_config = JinjaConfig(target_platform="linux-64")
            config = VariantConfig.from_file_with_context("variants.yaml", jinja_config)
            ```
        """
        variant_config = cls.__new__(cls)
        variant_config._inner = _VariantConfig.from_file_with_context(Path(path), jinja_config._config)
        return variant_config

    @classmethod
    def from_conda_build_config(cls, path: str | Path, jinja_config: JinjaConfig) -> "VariantConfig":
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
            ```python
            from rattler_build import JinjaConfig

            jinja_config = JinjaConfig(target_platform="linux-64")
            config = VariantConfig.from_conda_build_config("conda_build_config.yaml", jinja_config)
            ```
        """
        variant_config = cls.__new__(cls)
        variant_config._inner = _VariantConfig.from_conda_build_config(Path(path), jinja_config._config)
        return variant_config

    @classmethod
    def from_yaml(cls, yaml: str) -> "VariantConfig":
        """
        Load VariantConfig from a YAML string (variants.yaml format).

        Args:
            yaml: YAML string containing variant configuration

        Returns:
            A new VariantConfig instance

        Example:
            ```python
            yaml_str = '''
            python:
              - "3.8"
              - "3.9"
            '''
            config = VariantConfig.from_yaml(yaml_str)
            ```
        """
        variant_config = cls.__new__(cls)
        variant_config._inner = _VariantConfig.from_yaml(yaml)
        return variant_config

    @classmethod
    def from_yaml_with_context(cls, yaml: str, jinja_config: JinjaConfig) -> "VariantConfig":
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
            ```python
            from rattler_build import JinjaConfig

            yaml_str = '''
            c_compiler:
              - if: unix
                then: gcc
              - if: win
                then: msvc
            '''
            jinja_config = JinjaConfig(target_platform="linux-64")
            config = VariantConfig.from_yaml_with_context(yaml_str, jinja_config)
            ```
        """
        variant_config = cls.__new__(cls)
        variant_config._inner = _VariantConfig.from_yaml_with_context(yaml, jinja_config._config)
        return variant_config

    def keys(self) -> list[str]:
        """
        Get all variant keys.

        Returns:
            List of variant key names

        Example:
            ```python
            config = VariantConfig({"python": ["3.8", "3.9"], "numpy": ["1.21"]})
            config.keys()
            # ['numpy', 'python']
            ```
        """
        return self._inner.keys()

    @property
    def zip_keys(self) -> list[list[str]] | None:
        """
        Get zip_keys - groups of keys that should be zipped together.

        Zip keys ensure that certain variant keys are synchronized when creating
        combinations. For example, if python and numpy are zipped, then
        python=3.9 will always be paired with numpy=1.20, not with other numpy versions.

        Returns:
            List of groups (each group is a list of keys), or None if no zip keys are defined

        Example:
            ```python
            config = VariantConfig(
                {"python": ["3.9", "3.10"], "numpy": ["1.20", "1.21"]},
                zip_keys=[["python", "numpy"]]
            )
            len(config.combinations())  # 2, not 4
            # 2
            ```
        """
        return self._inner.zip_keys

    def get_values(self, key: str) -> list[Any] | None:
        """
        Get values for a specific variant key.

        Args:
            key: The variant key name

        Returns:
            List of values for the key, or None if key doesn't exist

        Example:
            ```python
            config = VariantConfig({"python": ["3.8", "3.9", "3.10"]})
            config.get_values("python")
            # ['3.8', '3.9', '3.10']
            ```
        """
        return self._inner.get_values(key)

    def to_dict(self) -> dict[str, list[Any]]:
        """
        Get all variants as a dictionary.

        Returns:
            Dictionary mapping variant keys to their value lists

        Example:
            ```python
            config = VariantConfig({"python": ["3.8", "3.9"]})
            config.to_dict()
            # {'python': ['3.8', '3.9']}
            ```
        """
        return self._inner.to_dict()

    def combinations(self) -> list[dict[str, Any]]:
        """
        Generate all combinations of variant values.

        Returns:
            List of dictionaries, each representing one variant combination

        Example:
            ```python
            config = VariantConfig({"python": ["3.8", "3.9"], "numpy": ["1.21", "1.22"]})
            combos = config.combinations()
            len(combos)
            # 4
            combos[0]
            # {'python': '3.8', 'numpy': '1.21'}
            ```
        """
        return self._inner.combinations()

    def __len__(self) -> int:
        """Get the number of variant keys."""
        return len(self._inner)

    def __getitem__(self, key: str) -> list[Any]:
        """
        Get values for a variant key using dict-like access.

        Args:
            key: The variant key name

        Returns:
            List of values for the key

        Raises:
            KeyError: If the key doesn't exist

        Example:
            ```python
            config = VariantConfig({"python": ["3.9", "3.10"]})
            config["python"]
            # ['3.9', '3.10']
            ```
        """
        values = self._inner.get_values(key)
        if values is None:
            raise KeyError(f"Variant key '{key}' not found")
        return values

    def __contains__(self, key: str) -> bool:
        """
        Check if a variant key exists.

        Args:
            key: The variant key name

        Returns:
            True if the key exists, False otherwise

        Example:
            ```python
            config = VariantConfig({"python": ["3.9"]})
            "python" in config
            # True
            "ruby" in config
            # False
            ```
        """
        return self._inner.get_values(key) is not None

    def __iter__(self) -> Iterator[str]:
        """
        Iterate over variant keys.

        Returns:
            Iterator over variant key names

        Example:
            ```python
            config = VariantConfig({"python": ["3.9"], "numpy": ["1.21"]})
            list(config)
            # ['numpy', 'python']
            ```
        """
        return iter(self.keys())

    def items(self) -> ItemsView[str, list[str]]:
        """
        Get all variant key-value pairs.

        Returns:
            Iterator of (key, values) tuples

        Example:
            ```python
            config = VariantConfig({"python": ["3.9", "3.10"]})
            dict(config.items())
            # {'python': ['3.9', '3.10']}
            ```
        """
        return self.to_dict().items()

    def values(self) -> ValuesView[list[str]]:
        """
        Get all variant value lists.

        Returns:
            Iterator of value lists

        Example:
            ```python
            config = VariantConfig({"python": ["3.9", "3.10"]})
            list(config.values())
            # [['3.9', '3.10']]
            ```
        """
        return self.to_dict().values()

    def get(self, key: str, default: list[Any] | None = None) -> list[Any] | None:
        """
        Get values for a variant key with a default.

        Args:
            key: The variant key name
            default: Default value if key doesn't exist

        Returns:
            List of values for the key, or default if key doesn't exist

        Example:
            ```python
            config = VariantConfig({"python": ["3.9"]})
            config.get("python")
            # ['3.9']
            config.get("ruby", ["2.7"])
            # ['2.7']
            ```
        """
        values = self._inner.get_values(key)
        return values if values is not None else default

    def __repr__(self) -> str:
        return repr(self._inner)

    def __str__(self) -> str:
        return f"VariantConfig(keys={self.keys()})"
