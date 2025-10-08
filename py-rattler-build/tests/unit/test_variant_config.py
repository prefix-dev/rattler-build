"""Test suite for variant_config module."""

from pathlib import Path
import pytest
from rattler_build import Pin, VariantConfig, SelectorConfig


class TestPin:
    """Test suite for Pin class."""

    def test_create_empty_pin(self):
        """Test creating an empty Pin."""
        pin = Pin()
        assert pin.max_pin is None
        assert pin.min_pin is None

    def test_create_pin_with_max_only(self):
        """Test creating a Pin with only max_pin."""
        pin = Pin(max_pin="x.x")
        assert pin.max_pin == "x.x"
        assert pin.min_pin is None

    def test_create_pin_with_min_only(self):
        """Test creating a Pin with only min_pin."""
        pin = Pin(min_pin="x.x.x")
        assert pin.max_pin is None
        assert pin.min_pin == "x.x.x"

    def test_create_pin_with_both(self):
        """Test creating a Pin with both max_pin and min_pin."""
        pin = Pin(max_pin="x.x", min_pin="x.x.x.x")
        assert pin.max_pin == "x.x"
        assert pin.min_pin == "x.x.x.x"

    def test_modify_max_pin(self):
        """Test modifying max_pin after creation."""
        pin = Pin()
        pin.max_pin = "x.x.x"
        assert pin.max_pin == "x.x.x"

    def test_modify_min_pin(self):
        """Test modifying min_pin after creation."""
        pin = Pin()
        pin.min_pin = "x.x"
        assert pin.min_pin == "x.x"

    def test_pin_equality(self):
        """Test Pin equality comparison."""
        pin1 = Pin(max_pin="x.x", min_pin="x.x.x")
        pin2 = Pin(max_pin="x.x", min_pin="x.x.x")
        pin3 = Pin(max_pin="x.x.x", min_pin="x.x.x")

        assert pin1 == pin2
        assert pin1 != pin3

    def test_pin_equality_with_none(self):
        """Test Pin equality when some fields are None."""
        pin1 = Pin(max_pin="x.x")
        pin2 = Pin(max_pin="x.x")
        pin3 = Pin(min_pin="x.x")

        assert pin1 == pin2
        assert pin1 != pin3

    def test_pin_repr(self):
        """Test Pin string representation."""
        pin = Pin(max_pin="x.x", min_pin="x.x.x")
        repr_str = repr(pin)
        assert "Pin" in repr_str
        assert "x.x" in repr_str
        assert "x.x.x" in repr_str

    def test_pin_not_equal_to_other_types(self):
        """Test that Pin is not equal to other types."""
        pin = Pin(max_pin="x.x")
        assert pin != "x.x"
        assert pin != 42
        assert pin != {"max_pin": "x.x"}


class TestVariantConfig:
    """Test suite for VariantConfig class."""

    def test_create_empty_config(self):
        """Test creating an empty VariantConfig."""
        config = VariantConfig()
        assert config.pin_run_as_build is None
        assert config.zip_keys is None
        assert config.variants == {}

    def test_create_config_with_variants(self):
        """Test creating a VariantConfig with variants."""
        config = VariantConfig(variants={"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.22"]})
        assert len(config.variants) == 2
        assert config.variants["python"] == ["3.9", "3.10", "3.11"]
        assert config.variants["numpy"] == ["1.21", "1.22"]

    def test_create_config_with_zip_keys(self):
        """Test creating a VariantConfig with zip_keys."""
        config = VariantConfig(zip_keys=[["python", "numpy"], ["cuda", "cudnn"]])
        assert config.zip_keys == [["python", "numpy"], ["cuda", "cudnn"]]

    def test_create_config_with_pin_run_as_build(self):
        """Test creating a VariantConfig with pin_run_as_build."""
        config = VariantConfig(
            pin_run_as_build={"python": Pin(max_pin="x.x"), "numpy": Pin(max_pin="x.x", min_pin="x.x.x.x")}
        )
        assert "python" in config.pin_run_as_build
        assert "numpy" in config.pin_run_as_build
        assert config.pin_run_as_build["python"].max_pin == "x.x"
        assert config.pin_run_as_build["numpy"].min_pin == "x.x.x.x"

    def test_modify_variants(self):
        """Test modifying variants after creation."""
        config = VariantConfig()
        config.variants = {"rust": ["1.70", "1.71"]}
        assert config.variants["rust"] == ["1.70", "1.71"]

    def test_modify_zip_keys(self):
        """Test modifying zip_keys after creation."""
        config = VariantConfig()
        config.zip_keys = [["cuda", "cudnn"]]
        assert config.zip_keys == [["cuda", "cudnn"]]

    def test_modify_pin_run_as_build(self):
        """Test modifying pin_run_as_build after creation."""
        config = VariantConfig()
        config.pin_run_as_build = {"go": Pin(max_pin="x.x")}
        assert config.pin_run_as_build["go"].max_pin == "x.x"

    def test_variant_config_equality(self):
        """Test VariantConfig equality comparison."""
        config1 = VariantConfig(variants={"python": ["3.9", "3.10"]}, zip_keys=[["python", "numpy"]])
        config2 = VariantConfig(variants={"python": ["3.9", "3.10"]}, zip_keys=[["python", "numpy"]])
        config3 = VariantConfig(variants={"python": ["3.9", "3.11"]}, zip_keys=[["python", "numpy"]])

        assert config1 == config2
        assert config1 != config3

    def test_variant_config_repr(self):
        """Test VariantConfig string representation."""
        config = VariantConfig(variants={"python": ["3.9"]}, pin_run_as_build={"numpy": Pin(max_pin="x.x")})
        repr_str = repr(config)
        assert "VariantConfig" in repr_str

    def test_variant_config_not_equal_to_other_types(self):
        """Test that VariantConfig is not equal to other types."""
        config = VariantConfig(variants={"python": ["3.9"]})
        assert config != "config"
        assert config != 42
        assert config != {"variants": {"python": ["3.9"]}}

    def test_variants_with_different_types(self):
        """Test variants with different value types."""
        config = VariantConfig(
            variants={"python": ["3.9", "3.10"], "cuda_enabled": [True, False], "cuda_version": [11, 12]}
        )
        assert config.variants["python"] == ["3.9", "3.10"]
        assert config.variants["cuda_enabled"] == [True, False]
        assert config.variants["cuda_version"] == [11, 12]

    def test_complex_config(self):
        """Test a complex VariantConfig with all fields."""
        config = VariantConfig(
            pin_run_as_build={"python": Pin(max_pin="x.x"), "numpy": Pin(max_pin="x.x", min_pin="x.x.x.x")},
            zip_keys=[["python", "numpy"]],
            variants={"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.22", "1.23"], "cuda": ["11.8", "12.0"]},
        )

        # Verify all fields are set correctly
        assert len(config.pin_run_as_build) == 2
        assert config.zip_keys == [["python", "numpy"]]
        assert len(config.variants) == 3
        assert config.variants["python"] == ["3.9", "3.10", "3.11"]

    def test_clear_pin_run_as_build(self):
        """Test setting pin_run_as_build to None."""
        config = VariantConfig(pin_run_as_build={"python": Pin(max_pin="x.x")})
        assert config.pin_run_as_build is not None

        config.pin_run_as_build = None
        assert config.pin_run_as_build is None

    def test_clear_zip_keys(self):
        """Test setting zip_keys to None."""
        config = VariantConfig(zip_keys=[["python", "numpy"]])
        assert config.zip_keys is not None

        config.zip_keys = None
        assert config.zip_keys is None

    def test_replace_variants(self):
        """Test completely replacing variants."""
        config = VariantConfig(variants={"python": ["3.9", "3.10"]})
        assert "python" in config.variants

        config.variants = {"rust": ["1.70", "1.71"]}
        assert "rust" in config.variants
        assert "python" not in config.variants

    def test_empty_variants_dict(self):
        """Test setting variants to an empty dict."""
        config = VariantConfig(variants={"python": ["3.9", "3.10"]})
        config.variants = {}
        assert config.variants == {}

    def test_multiple_zip_key_groups(self):
        """Test VariantConfig with multiple zip_key groups."""
        config = VariantConfig(zip_keys=[["python", "numpy"], ["cuda", "cudnn"], ["gcc", "gxx"]])
        assert len(config.zip_keys) == 3
        assert config.zip_keys[0] == ["python", "numpy"]
        assert config.zip_keys[1] == ["cuda", "cudnn"]
        assert config.zip_keys[2] == ["gcc", "gxx"]


class TestIntegration:
    """Integration tests for Pin and VariantConfig."""

    def test_pin_in_variant_config_round_trip(self):
        """Test that Pin objects survive round-trip through VariantConfig."""
        original_pin = Pin(max_pin="x.x", min_pin="x.x.x")
        config = VariantConfig(pin_run_as_build={"python": original_pin})

        retrieved_pin = config.pin_run_as_build["python"]
        assert retrieved_pin.max_pin == original_pin.max_pin
        assert retrieved_pin.min_pin == original_pin.min_pin

    def test_modify_pin_after_adding_to_config(self):
        """Test that modifying original Pin doesn't affect config."""
        pin = Pin(max_pin="x.x")
        config = VariantConfig(pin_run_as_build={"python": pin})

        # Modify the original pin
        pin.max_pin = "x.x.x"

        # Config should still have the original value
        # Note: This depends on whether we copy or reference
        assert config.pin_run_as_build["python"].max_pin == "x.x"

    def test_realistic_python_variant_config(self):
        """Test a realistic Python package variant configuration."""
        config = VariantConfig(
            pin_run_as_build={"python": Pin(max_pin="x.x"), "numpy": Pin(max_pin="x.x")},
            zip_keys=[["python", "numpy"]],
            variants={"python": ["3.9", "3.10", "3.11", "3.12"], "numpy": ["1.21", "1.22", "1.23", "1.24"]},
        )

        assert len(config.variants["python"]) == 4
        assert len(config.variants["numpy"]) == 4
        assert config.zip_keys == [["python", "numpy"]]

    def test_realistic_cuda_variant_config(self):
        """Test a realistic CUDA variant configuration."""
        config = VariantConfig(
            zip_keys=[["cuda_compiler_version", "cudnn"]],
            variants={
                "cuda_compiler_version": ["11.8", "12.0"],
                "cudnn": ["8.6", "8.8"],
                "python": ["3.9", "3.10", "3.11"],
            },
        )

        # 2 CUDA versions * 3 Python versions = 6 total variants
        # (with cudnn zipped to cuda_compiler_version)
        assert len(config.variants["cuda_compiler_version"]) == 2
        assert len(config.variants["cudnn"]) == 2
        assert len(config.variants["python"]) == 3


class TestMerge:
    """Test suite for VariantConfig.merge() method."""

    def test_merge_variants(self):
        """Test merging variants from two configs."""
        config1 = VariantConfig(variants={"python": ["3.9"], "numpy": ["1.21"]})
        config2 = VariantConfig(variants={"cuda": ["11.8"]})

        config1.merge(config2)

        assert "python" in config1.variants
        assert "numpy" in config1.variants
        assert "cuda" in config1.variants
        assert config1.variants["cuda"] == ["11.8"]

    def test_merge_replaces_existing_keys(self):
        """Test that merge replaces existing variant keys."""
        config1 = VariantConfig(variants={"python": ["3.9"]})
        config2 = VariantConfig(variants={"python": ["3.10", "3.11"]})

        config1.merge(config2)

        assert config1.variants["python"] == ["3.10", "3.11"]

    def test_merge_pin_run_as_build(self):
        """Test merging pin_run_as_build."""
        config1 = VariantConfig(pin_run_as_build={"python": Pin(max_pin="x.x")})
        config2 = VariantConfig(pin_run_as_build={"numpy": Pin(max_pin="x.x")})

        config1.merge(config2)

        assert "python" in config1.pin_run_as_build
        assert "numpy" in config1.pin_run_as_build

    def test_merge_replaces_zip_keys(self):
        """Test that merge replaces (not merges) zip_keys."""
        config1 = VariantConfig(zip_keys=[["python", "numpy"]])
        config2 = VariantConfig(zip_keys=[["cuda", "cudnn"]])

        config1.merge(config2)

        assert config1.zip_keys == [["cuda", "cudnn"]]

    def test_merge_with_none_zip_keys(self):
        """Test merging when one config has no zip_keys."""
        config1 = VariantConfig(zip_keys=[["python", "numpy"]])
        config2 = VariantConfig(variants={"cuda": ["11.8"]})

        config1.merge(config2)

        # According to Rust implementation, zip_keys are replaced even if None
        # This matches the documented behavior: "zip_keys are replaced (not merged)"
        assert config1.zip_keys is None

    def test_merge_modifies_in_place(self):
        """Test that merge modifies the config in-place."""
        config1 = VariantConfig(variants={"python": ["3.9"]})
        config2 = VariantConfig(variants={"numpy": ["1.21"]})

        original_id = id(config1)
        config1.merge(config2)

        assert id(config1) == original_id


class TestFileLoading:
    """Test suite for loading variant configs from files."""

    @pytest.fixture
    def variant_configs_dir(self) -> Path:
        """Get the path to test variant config files."""
        return Path(__file__).parent.parent / "data" / "variant_configs"

    def test_load_simple_variants(self, variant_configs_dir: Path):
        """Test loading a simple variants file."""
        config = VariantConfig.from_files([variant_configs_dir / "simple_variants.yaml"])

        assert "python" in config.variants
        assert "numpy" in config.variants
        assert config.variants["python"] == ["3.9", "3.10", "3.11"]
        assert config.variants["numpy"] == ["1.21", "1.22", "1.23"]

    def test_load_with_zip_keys(self, variant_configs_dir: Path):
        """Test loading a config file with zip_keys."""
        config = VariantConfig.from_files([variant_configs_dir / "with_zip_keys.yaml"])

        assert "python" in config.variants
        assert "numpy" in config.variants
        assert config.zip_keys == [["python", "numpy"]]

    def test_load_conda_build_config(self, variant_configs_dir: Path):
        """Test loading a conda_build_config.yaml file."""
        config = VariantConfig.from_files([variant_configs_dir / "conda_build_config.yaml"])

        assert "python" in config.variants
        assert "cuda_compiler_version" in config.variants
        assert config.variants["python"] == ["3.9", "3.10"]
        assert config.variants["cuda_compiler_version"] == ["11.8", "12.0"]

        # Check pin_run_as_build
        assert config.pin_run_as_build is not None
        assert "python" in config.pin_run_as_build
        assert config.pin_run_as_build["python"].max_pin == "x.x"
        assert "numpy" in config.pin_run_as_build
        assert config.pin_run_as_build["numpy"].max_pin == "x.x"

    def test_load_multiple_files_merge(self, variant_configs_dir: Path):
        """Test loading and merging multiple variant config files."""
        config = VariantConfig.from_files(
            [variant_configs_dir / "simple_variants.yaml", variant_configs_dir / "override_variants.yaml"]
        )

        # Python should be overridden by second file
        assert config.variants["python"] == ["3.12"]
        # Numpy should still be from first file
        assert config.variants["numpy"] == ["1.21", "1.22", "1.23"]
        # Rust should be from second file
        assert config.variants["rust"] == ["1.70", "1.71"]

    def test_load_with_selector_config(self, variant_configs_dir: Path):
        """Test loading with a specific SelectorConfig."""
        selector_config = SelectorConfig(target_platform="linux-64")
        config = VariantConfig.from_files(
            [variant_configs_dir / "simple_variants.yaml"], selector_config=selector_config
        )

        assert "python" in config.variants

    def test_load_with_string_paths(self, variant_configs_dir: Path):
        """Test loading with string paths instead of Path objects."""
        config = VariantConfig.from_files([str(variant_configs_dir / "simple_variants.yaml")])

        assert "python" in config.variants
        assert config.variants["python"] == ["3.9", "3.10", "3.11"]

    def test_load_nonexistent_file(self, variant_configs_dir: Path):
        """Test that loading a nonexistent file raises an error."""
        with pytest.raises(Exception):  # RattlerBuildError
            VariantConfig.from_files([variant_configs_dir / "nonexistent.yaml"])

    def test_load_empty_list(self):
        """Test loading with an empty file list."""
        config = VariantConfig.from_files([])
        # from_files with empty list returns an empty config (no files to process)
        # target_platform and build_platform are only added by from_file, not from_files
        assert config.variants == {}
        assert config.zip_keys is None
        assert config.pin_run_as_build is None

    def test_from_file_single(self, variant_configs_dir: Path):
        """Test loading a single file with from_file method."""
        config = VariantConfig.from_file(variant_configs_dir / "simple_variants.yaml")

        assert "python" in config.variants
        assert "numpy" in config.variants
        # Note: from_file also adds target_platform and build_platform
        assert "target_platform" in config.variants
        assert "build_platform" in config.variants

    def test_from_file_with_string_path(self, variant_configs_dir: Path):
        """Test from_file with string path instead of Path object."""
        config = VariantConfig.from_file(str(variant_configs_dir / "simple_variants.yaml"))

        assert "python" in config.variants
        assert config.variants["python"] == ["3.9", "3.10", "3.11"]

    def test_from_file_conda_build_config(self, variant_configs_dir: Path):
        """Test from_file with conda_build_config.yaml."""
        config = VariantConfig.from_file(variant_configs_dir / "conda_build_config.yaml")

        assert "python" in config.variants
        assert config.pin_run_as_build is not None
        assert "python" in config.pin_run_as_build

    def test_from_file_with_selector(self, variant_configs_dir: Path):
        """Test from_file with a specific SelectorConfig."""
        selector = SelectorConfig(target_platform="linux-64")
        config = VariantConfig.from_file(variant_configs_dir / "simple_variants.yaml", selector_config=selector)

        assert "python" in config.variants
        assert config.variants["target_platform"] == ["linux-64"]

    def test_manual_merge_workflow(self, variant_configs_dir: Path):
        """Test manually loading and merging configs."""
        config1 = VariantConfig.from_file(variant_configs_dir / "simple_variants.yaml")
        config2 = VariantConfig.from_file(variant_configs_dir / "override_variants.yaml")

        # Merge config2 into config1
        config1.merge(config2)

        # Python should be overridden
        assert config1.variants["python"] == ["3.12"]
        # Rust should be added
        assert config1.variants["rust"] == ["1.70", "1.71"]
        # Numpy should remain from config1
        assert config1.variants["numpy"] == ["1.21", "1.22", "1.23"]
