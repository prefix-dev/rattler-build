"""Test suite for SandboxConfig."""

from pathlib import Path
from rattler_build import SandboxConfig


class TestSandboxConfig:
    """Test suite for SandboxConfig class."""

    def test_create_default_config(self) -> None:
        """Test creating a default SandboxConfig."""
        config = SandboxConfig()
        assert config.allow_network is False
        assert config.read == []
        assert config.read_execute == []
        assert config.read_write == []

    def test_create_config_with_network(self) -> None:
        """Test creating a SandboxConfig with network access."""
        config = SandboxConfig(allow_network=True)
        assert config.allow_network is True

    def test_create_config_with_paths(self) -> None:
        """Test creating a SandboxConfig with paths."""
        config = SandboxConfig(
            read=[Path("/usr"), Path("/etc")],
            read_execute=[Path("/bin"), Path("/usr/bin")],
            read_write=[Path("/tmp")],
        )
        assert len(config.read) == 2
        assert Path("/usr") in config.read
        assert Path("/etc") in config.read
        assert len(config.read_execute) == 2
        assert Path("/bin") in config.read_execute
        assert Path("/usr/bin") in config.read_execute
        assert len(config.read_write) == 1
        assert Path("/tmp") in config.read_write

    def test_modify_allow_network(self) -> None:
        """Test modifying allow_network after creation."""
        config = SandboxConfig()
        assert config.allow_network is False
        config.allow_network = True
        assert config.allow_network is True

    def test_modify_read_paths(self) -> None:
        """Test modifying read paths after creation."""
        config = SandboxConfig()
        config.read = [Path("/usr/local")]
        assert len(config.read) == 1
        assert Path("/usr/local") in config.read

    def test_modify_read_execute_paths(self) -> None:
        """Test modifying read_execute paths after creation."""
        config = SandboxConfig()
        config.read_execute = [Path("/usr/local/bin")]
        assert len(config.read_execute) == 1
        assert Path("/usr/local/bin") in config.read_execute

    def test_modify_read_write_paths(self) -> None:
        """Test modifying read_write paths after creation."""
        config = SandboxConfig()
        config.read_write = [Path("/var/tmp")]
        assert len(config.read_write) == 1
        assert Path("/var/tmp") in config.read_write

    def test_add_read_path(self) -> None:
        """Test adding a read path."""
        config = SandboxConfig()
        config.add_read(Path("/usr"))
        assert Path("/usr") in config.read

    def test_add_read_execute_path(self) -> None:
        """Test adding a read_execute path."""
        config = SandboxConfig()
        config.add_read_execute(Path("/bin"))
        assert Path("/bin") in config.read_execute

    def test_add_read_write_path(self) -> None:
        """Test adding a read_write path."""
        config = SandboxConfig()
        config.add_read_write(Path("/tmp"))
        assert Path("/tmp") in config.read_write

    def test_add_multiple_paths(self) -> None:
        """Test adding multiple paths."""
        config = SandboxConfig()
        config.add_read(Path("/usr"))
        config.add_read(Path("/etc"))
        assert len(config.read) == 2
        assert Path("/usr") in config.read
        assert Path("/etc") in config.read

    def test_repr(self) -> None:
        """Test string representation."""
        config = SandboxConfig(allow_network=True)
        repr_str = repr(config)
        assert "SandboxConfig" in repr_str
        assert "allow_network=True" in repr_str

    def test_str(self) -> None:
        """Test detailed string representation."""
        config = SandboxConfig(allow_network=True)
        str_repr = str(config)
        assert "Sandbox Configuration" in str_repr or "SandboxConfig" in str_repr


class TestSandboxConfigPlatformDefaults:
    """Test suite for platform-specific default configurations."""

    def test_for_macos(self) -> None:
        """Test macOS default configuration."""
        config = SandboxConfig.for_macos()
        assert config.allow_network is False
        assert len(config.read) > 0
        assert Path("/") in config.read
        assert len(config.read_execute) > 0
        # macOS should have /bin and /usr/bin
        read_execute_strs = [str(p) for p in config.read_execute]
        assert any("/bin" in p for p in read_execute_strs)
        assert len(config.read_write) > 0
        # Should have /tmp
        read_write_strs = [str(p) for p in config.read_write]
        assert any("/tmp" in p for p in read_write_strs)

    def test_for_linux(self) -> None:
        """Test Linux default configuration."""
        config = SandboxConfig.for_linux()
        assert config.allow_network is False
        assert len(config.read) > 0
        assert Path("/") in config.read
        assert len(config.read_execute) > 0
        # Linux should have /bin, /usr/bin, and lib directories
        read_execute_strs = [str(p) for p in config.read_execute]
        assert any("/bin" in p for p in read_execute_strs)
        assert len(config.read_write) > 0
        # Should have /tmp
        read_write_strs = [str(p) for p in config.read_write]
        assert any("/tmp" in p for p in read_write_strs)

    def test_modify_platform_defaults(self) -> None:
        """Test modifying platform default configurations."""
        config = SandboxConfig.for_linux()
        config.allow_network = True
        assert config.allow_network is True

        config.add_read_write(Path("/my/custom/path"))
        assert Path("/my/custom/path") in config.read_write


class TestSandboxConfigIntegration:
    """Integration tests for SandboxConfig."""

    def test_full_workflow(self) -> None:
        """Test a complete workflow of creating and modifying a config."""
        # Start with platform defaults
        config = SandboxConfig.for_linux()

        # Enable network for this build
        config.allow_network = True

        # Add custom paths
        config.add_read(Path("/opt/custom"))
        config.add_read_execute(Path("/opt/custom/bin"))
        config.add_read_write(Path("/workspace"))

        # Verify everything
        assert config.allow_network is True
        assert Path("/opt/custom") in config.read
        assert Path("/opt/custom/bin") in config.read_execute
        assert Path("/workspace") in config.read_write

    def test_replace_paths_completely(self) -> None:
        """Test completely replacing path lists."""
        config = SandboxConfig.for_linux()

        original_read_count = len(config.read)
        assert original_read_count > 0

        # Replace with custom paths
        config.read = [Path("/custom/path")]
        assert len(config.read) == 1
        assert Path("/custom/path") in config.read

    def test_clear_paths(self) -> None:
        """Test clearing all paths."""
        config = SandboxConfig(read=[Path("/usr")], read_execute=[Path("/bin")], read_write=[Path("/tmp")])

        config.read = []
        config.read_execute = []
        config.read_write = []

        assert config.read == []
        assert config.read_execute == []
        assert config.read_write == []

    def test_realistic_build_config(self) -> None:
        """Test a realistic build configuration."""
        config = SandboxConfig.for_linux()

        # Disable network (security best practice)
        config.allow_network = False

        # Add project-specific paths
        project_root = Path("/home/user/project")
        config.add_read(project_root)
        config.add_read_execute(project_root / "scripts")
        config.add_read_write(project_root / "build")

        assert Path("/home/user/project") in config.read
        assert Path("/home/user/project/scripts") in config.read_execute
        assert Path("/home/user/project/build") in config.read_write


class TestSandboxConfigEdgeCases:
    """Test edge cases and error handling."""

    def test_empty_path_lists(self) -> None:
        """Test with empty path lists."""
        config = SandboxConfig(read=[], read_execute=[], read_write=[])
        assert config.read == []
        assert config.read_execute == []
        assert config.read_write == []

    def test_duplicate_paths(self) -> None:
        """Test adding duplicate paths."""
        config = SandboxConfig()
        config.add_read(Path("/usr"))
        config.add_read(Path("/usr"))
        # Should have duplicates (no deduplication)
        assert len(config.read) == 2

    def test_relative_paths(self) -> None:
        """Test with relative paths."""
        config = SandboxConfig()
        config.add_read(Path("relative/path"))
        assert Path("relative/path") in config.read

    def test_windows_paths(self) -> None:
        """Test with Windows-style paths."""
        config = SandboxConfig()
        # Path should handle Windows paths on Windows
        config.add_read(Path("C:/Users/test"))
        assert len(config.read) == 1
