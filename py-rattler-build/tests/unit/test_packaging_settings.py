"""Test suite for PackagingConfig."""

import pytest
from rattler_build import PackagingConfig, ArchiveType


class TestArchiveType:
    """Test suite for ArchiveType enum."""

    def test_archive_type_values(self) -> None:
        """Test that ArchiveType has expected values."""
        assert hasattr(ArchiveType, "TarBz2")
        assert hasattr(ArchiveType, "Conda")

    def test_archive_type_extension(self) -> None:
        """Test archive type extensions."""
        # ArchiveType is an enum, access the actual PyO3 values
        tar_bz2 = ArchiveType.TarBz2.value
        conda = ArchiveType.Conda.value

        assert tar_bz2.extension() == ".tar.bz2"
        assert conda.extension() == ".conda"

    def test_archive_type_str(self) -> None:
        """Test archive type string representation."""
        tar_bz2 = ArchiveType.TarBz2.value
        conda = ArchiveType.Conda.value

        assert str(tar_bz2) == "tar.bz2"
        assert str(conda) == "conda"

    def test_archive_type_repr(self) -> None:
        """Test archive type repr."""
        tar_bz2 = ArchiveType.TarBz2.value
        conda = ArchiveType.Conda.value

        assert "TarBz2" in repr(tar_bz2)
        assert "Conda" in repr(conda)


class TestPackagingConfigCreation:
    """Test suite for PackagingConfig creation."""

    def test_create_tar_bz2_default(self) -> None:
        """Test creating tar.bz2 settings with default compression."""
        settings = PackagingConfig.tar_bz2()
        assert settings.is_tar_bz2()
        assert not settings.is_conda()
        assert settings.compression_level == 9
        assert settings.extension() == ".tar.bz2"

    def test_create_conda_default(self) -> None:
        """Test creating conda settings with default compression."""
        settings = PackagingConfig.conda()
        assert settings.is_conda()
        assert not settings.is_tar_bz2()
        assert settings.compression_level == 22
        assert settings.extension() == ".conda"

    def test_create_tar_bz2_custom_compression(self) -> None:
        """Test creating tar.bz2 settings with custom compression."""
        settings = PackagingConfig.tar_bz2(compression_level=5)
        assert settings.is_tar_bz2()
        assert settings.compression_level == 5

    def test_create_conda_custom_compression(self) -> None:
        """Test creating conda settings with custom compression."""
        settings = PackagingConfig.conda(compression_level=10)
        assert settings.is_conda()
        assert settings.compression_level == 10

    def test_create_with_constructor_tar_bz2(self) -> None:
        """Test creating settings with constructor for tar.bz2."""
        settings = PackagingConfig(ArchiveType.TarBz2.value, compression_level=7)
        assert settings.is_tar_bz2()
        assert settings.compression_level == 7

    def test_create_with_constructor_conda(self) -> None:
        """Test creating settings with constructor for conda."""
        settings = PackagingConfig(ArchiveType.Conda.value, compression_level=15)
        assert settings.is_conda()
        assert settings.compression_level == 15

    def test_create_with_default_compression(self) -> None:
        """Test that None compression_level uses appropriate defaults."""
        tar_settings = PackagingConfig(ArchiveType.TarBz2.value)
        assert tar_settings.compression_level == 9

        conda_settings = PackagingConfig(ArchiveType.Conda.value)
        assert conda_settings.compression_level == 22


class TestPackagingConfigValidation:
    """Test suite for compression level validation."""

    def test_tar_bz2_valid_range(self) -> None:
        """Test valid compression levels for tar.bz2."""
        for level in range(1, 10):  # 1-9 inclusive
            settings = PackagingConfig.tar_bz2(compression_level=level)
            assert settings.compression_level == level

    def test_tar_bz2_invalid_low(self) -> None:
        """Test invalid low compression level for tar.bz2."""
        with pytest.raises(Exception):  # RattlerBuildError
            PackagingConfig.tar_bz2(compression_level=0)

    def test_tar_bz2_invalid_high(self) -> None:
        """Test invalid high compression level for tar.bz2."""
        with pytest.raises(Exception):  # RattlerBuildError
            PackagingConfig.tar_bz2(compression_level=10)

    def test_conda_valid_range(self) -> None:
        """Test valid compression levels for conda."""
        # Test some values in the range -7 to 22
        for level in [-7, -1, 0, 1, 10, 15, 20, 22]:
            settings = PackagingConfig.conda(compression_level=level)
            assert settings.compression_level == level

    def test_conda_invalid_low(self) -> None:
        """Test invalid low compression level for conda."""
        with pytest.raises(Exception):  # RattlerBuildError
            PackagingConfig.conda(compression_level=-8)

    def test_conda_invalid_high(self) -> None:
        """Test invalid high compression level for conda."""
        with pytest.raises(Exception):  # RattlerBuildError
            PackagingConfig.conda(compression_level=23)


class TestPackagingConfigModification:
    """Test suite for modifying PackagingConfig."""

    def test_modify_compression_level_tar_bz2(self) -> None:
        """Test modifying compression level for tar.bz2."""
        settings = PackagingConfig.tar_bz2()
        settings.compression_level = 5
        assert settings.compression_level == 5

    def test_modify_compression_level_conda(self) -> None:
        """Test modifying compression level for conda."""
        settings = PackagingConfig.conda()
        settings.compression_level = 10
        assert settings.compression_level == 10

    def test_modify_compression_level_validates_tar_bz2(self) -> None:
        """Test that setting compression level validates for tar.bz2."""
        settings = PackagingConfig.tar_bz2()
        with pytest.raises(Exception):  # RattlerBuildError
            settings.compression_level = 10

    def test_modify_compression_level_validates_conda(self) -> None:
        """Test that setting compression level validates for conda."""
        settings = PackagingConfig.conda()
        with pytest.raises(Exception):  # RattlerBuildError
            settings.compression_level = 23

    def test_change_archive_type(self) -> None:
        """Test changing archive type."""
        settings = PackagingConfig.tar_bz2()
        assert settings.is_tar_bz2()

        settings.archive_type = ArchiveType.Conda.value
        assert settings.is_conda()
        assert settings.extension() == ".conda"

    def test_change_archive_type_validates_compression(self) -> None:
        """Test that changing archive type doesn't auto-validate compression."""
        # Start with conda format with compression level 15
        settings = PackagingConfig.conda(compression_level=15)
        assert settings.compression_level == 15

        # Change to tar.bz2 - compression level 15 is invalid for tar.bz2
        settings.archive_type = ArchiveType.TarBz2.value

        # The compression level should still be 15 (no auto-adjustment)
        # But trying to set it again should fail
        with pytest.raises(Exception):  # RattlerBuildError
            settings.compression_level = 15


class TestPackagingConfigProperties:
    """Test suite for PackagingConfig properties."""

    def test_archive_type_property(self) -> None:
        """Test archive_type property."""
        tar_settings = PackagingConfig.tar_bz2()
        assert tar_settings.archive_type.extension() == ".tar.bz2"

        conda_settings = PackagingConfig.conda()
        assert conda_settings.archive_type.extension() == ".conda"

    def test_compression_level_property(self) -> None:
        """Test compression_level property."""
        settings = PackagingConfig.tar_bz2(compression_level=5)
        assert settings.compression_level == 5

        settings.compression_level = 7
        assert settings.compression_level == 7

    def test_extension_method(self) -> None:
        """Test extension() method."""
        assert PackagingConfig.tar_bz2().extension() == ".tar.bz2"
        assert PackagingConfig.conda().extension() == ".conda"

    def test_is_tar_bz2_method(self) -> None:
        """Test is_tar_bz2() method."""
        assert PackagingConfig.tar_bz2().is_tar_bz2()
        assert not PackagingConfig.conda().is_tar_bz2()

    def test_is_conda_method(self) -> None:
        """Test is_conda() method."""
        assert PackagingConfig.conda().is_conda()
        assert not PackagingConfig.tar_bz2().is_conda()


class TestPackagingConfigStringRepresentation:
    """Test suite for string representations."""

    def test_repr_tar_bz2(self) -> None:
        """Test __repr__ for tar.bz2."""
        settings = PackagingConfig.tar_bz2(compression_level=5)
        repr_str = repr(settings)
        assert "PackagingConfig" in repr_str
        assert "TarBz2" in repr_str
        assert "5" in repr_str

    def test_repr_conda(self) -> None:
        """Test __repr__ for conda."""
        settings = PackagingConfig.conda(compression_level=15)
        repr_str = repr(settings)
        assert "PackagingConfig" in repr_str
        assert "Conda" in repr_str
        assert "15" in repr_str

    def test_str_tar_bz2(self) -> None:
        """Test __str__ for tar.bz2."""
        settings = PackagingConfig.tar_bz2(compression_level=7)
        str_repr = str(settings)
        assert "tar.bz2" in str_repr
        assert "7" in str_repr

    def test_str_conda(self) -> None:
        """Test __str__ for conda."""
        settings = PackagingConfig.conda(compression_level=10)
        str_repr = str(settings)
        assert "conda" in str_repr
        assert "10" in str_repr


class TestPackagingConfigIntegration:
    """Integration tests for PackagingConfig."""

    def test_fast_compression_workflow(self) -> None:
        """Test workflow for fast compression."""
        # Use fast compression for development builds
        settings = PackagingConfig.conda(compression_level=1)
        assert settings.is_conda()
        assert settings.compression_level == 1
        assert settings.extension() == ".conda"

    def test_max_compression_workflow(self) -> None:
        """Test workflow for maximum compression."""
        # Use maximum compression for release builds
        settings = PackagingConfig.conda(compression_level=22)
        assert settings.is_conda()
        assert settings.compression_level == 22

    def test_legacy_format_workflow(self) -> None:
        """Test workflow for legacy tar.bz2 format."""
        # Use tar.bz2 for compatibility
        settings = PackagingConfig.tar_bz2()
        assert settings.is_tar_bz2()
        assert settings.compression_level == 9

    def test_modify_for_different_use_case(self) -> None:
        """Test modifying settings for different use cases."""
        # Start with fast development settings
        settings = PackagingConfig.conda(compression_level=1)

        # Switch to release settings
        settings.compression_level = 22
        assert settings.compression_level == 22

    def test_format_switching(self) -> None:
        """Test switching between formats."""
        settings = PackagingConfig.conda()

        # Switch to tar.bz2
        settings.archive_type = ArchiveType.TarBz2.value
        settings.compression_level = 9  # Valid for tar.bz2

        assert settings.is_tar_bz2()
        assert settings.compression_level == 9
        assert settings.extension() == ".tar.bz2"

        # Switch back to conda
        settings.archive_type = ArchiveType.Conda.value
        settings.compression_level = 15  # Valid for conda

        assert settings.is_conda()
        assert settings.compression_level == 15
        assert settings.extension() == ".conda"


class TestPackagingConfigEdgeCases:
    """Test edge cases for PackagingConfig."""

    def test_boundary_values_tar_bz2(self) -> None:
        """Test boundary values for tar.bz2."""
        # Minimum valid
        min_settings = PackagingConfig.tar_bz2(compression_level=1)
        assert min_settings.compression_level == 1

        # Maximum valid
        max_settings = PackagingConfig.tar_bz2(compression_level=9)
        assert max_settings.compression_level == 9

    def test_boundary_values_conda(self) -> None:
        """Test boundary values for conda."""
        # Minimum valid
        min_settings = PackagingConfig.conda(compression_level=-7)
        assert min_settings.compression_level == -7

        # Maximum valid
        max_settings = PackagingConfig.conda(compression_level=22)
        assert max_settings.compression_level == 22

    def test_negative_compression_conda(self) -> None:
        """Test negative compression levels for conda."""
        # Negative values are valid for conda (faster, less compression)
        settings = PackagingConfig.conda(compression_level=-5)
        assert settings.compression_level == -5

    def test_recommended_settings(self) -> None:
        """Test recommended settings for production use."""
        # Recommended: conda format with high compression
        settings = PackagingConfig.conda()  # Default is 22
        assert settings.is_conda()
        assert settings.compression_level == 22
