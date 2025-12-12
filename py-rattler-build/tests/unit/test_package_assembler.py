"""Tests for the package builder (assemble_package) API."""

import hashlib
import zipfile
from datetime import datetime, timezone
from pathlib import Path

import pytest

from rattler_build import (
    ArchiveType,
    FileEntry,
    PackageOutput,
    RattlerBuildError,
    assemble_package,
    collect_files,
)


class TestAssemblePackageBasic:
    """Basic tests for assemble_package function."""

    def test_creates_conda_package(self, tmp_path: Path) -> None:
        """Verify the created .conda file exists and has correct structure."""
        files_dir = tmp_path / "files"
        files_dir.mkdir()
        (files_dir / "test.txt").write_text("hello world")

        output_dir = tmp_path / "output"
        output_dir.mkdir()

        output = assemble_package(
            name="testpkg",
            version="1.0.0",
            target_platform="linux-64",
            build_string="0",
            output_dir=output_dir,
            files_dir=files_dir,
        )

        # Check output type
        assert isinstance(output, PackageOutput)
        assert output.path.exists()
        assert output.path.suffix == ".conda"
        assert output.identifier == "testpkg-1.0.0-0"

        # Verify it's a valid conda package (zip with inner archives)
        with zipfile.ZipFile(output.path, "r") as zf:
            names = zf.namelist()
            # .conda format contains pkg-*.tar.zst and info-*.tar.zst
            assert any("pkg-" in n for n in names)
            assert any("info-" in n for n in names)

    def test_creates_tarbz2_package(self, tmp_path: Path) -> None:
        """Verify .tar.bz2 format works."""
        files_dir = tmp_path / "files"
        files_dir.mkdir()
        (files_dir / "data.txt").write_text("content")

        output_dir = tmp_path / "output"
        output_dir.mkdir()

        output = assemble_package(
            name="legacy",
            version="1.0.0",
            target_platform="linux-64",
            build_string="0",
            output_dir=output_dir,
            files_dir=files_dir,
            archive_type=ArchiveType.TarBz2,
        )

        assert output.path.suffix == ".bz2"
        assert ".tar" in output.path.name


class TestAssemblePackageMetadata:
    """Tests for package metadata in assemble_package."""

    def test_dependencies_in_package(self, tmp_path: Path) -> None:
        """Verify dependencies appear correctly in final package."""
        files_dir = tmp_path / "files"
        files_dir.mkdir()
        (files_dir / "lib.py").write_text("# library")

        output_dir = tmp_path / "output"
        output_dir.mkdir()

        output = assemble_package(
            name="with-deps",
            version="2.0.0",
            target_platform="linux-64",
            build_string="py312_0",
            output_dir=output_dir,
            files_dir=files_dir,
            depends=["python >=3.12", "numpy >=1.20"],
            constrains=["scipy >=1.0"],
            build_number=5,
        )

        # Extract and check index.json
        import json
        import tarfile
        import tempfile

        import zstandard

        with zipfile.ZipFile(output.path, "r") as zf:
            # Find the info archive
            info_name = [n for n in zf.namelist() if n.startswith("info-")][0]
            with zf.open(info_name) as info_zst:
                dctx = zstandard.ZstdDecompressor()
                with tempfile.NamedTemporaryFile() as tmp:
                    tmp.write(dctx.decompress(info_zst.read()))
                    tmp.flush()
                    tmp.seek(0)
                    with tarfile.open(fileobj=tmp, mode="r:") as tf:
                        index_data = tf.extractfile("info/index.json")
                        assert index_data is not None
                        index = json.load(index_data)

        assert index["name"] == "with-deps"
        assert index["version"] == "2.0.0"
        assert index["build"] == "py312_0"
        assert index["build_number"] == 5
        assert "python >=3.12" in index["depends"]
        assert "numpy >=1.20" in index["depends"]
        assert "scipy >=1.0" in index["constrains"]


class TestCollectFiles:
    """Tests for collect_files."""

    def test_collects_all_files(self, tmp_path: Path) -> None:
        """Verify collector finds all files by default."""
        (tmp_path / "a.txt").write_text("a")
        (tmp_path / "b.py").write_text("b")
        (tmp_path / "subdir").mkdir()
        (tmp_path / "subdir" / "c.txt").write_text("c")

        files = collect_files(tmp_path)

        assert len(files) == 3
        destinations = {str(f.destination) for f in files}
        assert "a.txt" in destinations
        assert "b.py" in destinations

    def test_exclude_pattern(self, tmp_path: Path) -> None:
        """Verify exclude globs filter out files."""
        (tmp_path / "keep.py").write_text("keep")
        (tmp_path / "remove.pyc").write_text("remove")
        (tmp_path / "__pycache__").mkdir()
        (tmp_path / "__pycache__" / "cached.pyc").write_text("cached")

        files = collect_files(
            tmp_path,
            exclude_globs=["**/*.pyc", "**/__pycache__/**"],
        )

        # Should only have keep.py
        assert len(files) == 1
        assert files[0].destination.name == "keep.py"

    def test_include_pattern(self, tmp_path: Path) -> None:
        """Verify include globs select specific files."""
        (tmp_path / "code.py").write_text("code")
        (tmp_path / "data.json").write_text("{}")
        (tmp_path / "readme.md").write_text("# readme")

        files = collect_files(tmp_path, include_globs=["**/*.py"])

        assert len(files) == 1
        assert files[0].destination.name == "code.py"


class TestFileEntry:
    """Tests for FileEntry."""

    def test_from_paths(self, tmp_path: Path) -> None:
        """Test creating FileEntry from paths."""
        src = tmp_path / "source.txt"
        src.write_text("content")

        entry = FileEntry.from_paths(src, Path("dest.txt"))

        assert entry.source == src
        assert entry.destination == Path("dest.txt")
        assert not entry.is_symlink


class TestReproducibleBuilds:
    """Tests for reproducible builds with timestamps."""

    def test_same_timestamp_produces_identical_packages(self, tmp_path: Path) -> None:
        """Verify same inputs with same timestamp produce identical packages."""
        files_dir = tmp_path / "files"
        files_dir.mkdir()
        (files_dir / "data.txt").write_text("deterministic content")

        output_dir = tmp_path / "output"
        output_dir.mkdir()

        # Fixed timestamp: 2024-01-01 00:00:00 UTC
        timestamp = datetime(2024, 1, 1, 0, 0, 0, tzinfo=timezone.utc)

        output1 = assemble_package(
            name="repro",
            version="1.0.0",
            target_platform="noarch",
            build_string="0",
            output_dir=output_dir,
            files_dir=files_dir,
            timestamp=timestamp,
            noarch="generic",
        )

        # Rename first package
        first_path = output_dir / "first.conda"
        output1.path.rename(first_path)

        output2 = assemble_package(
            name="repro",
            version="1.0.0",
            target_platform="noarch",
            build_string="0",
            output_dir=output_dir,
            files_dir=files_dir,
            timestamp=timestamp,
            noarch="generic",
        )

        hash1 = hashlib.sha256(first_path.read_bytes()).hexdigest()
        hash2 = hashlib.sha256(output2.path.read_bytes()).hexdigest()

        assert hash1 == hash2, "Packages with same timestamp should be identical"


class TestArchiveType:
    """Tests for ArchiveType enum."""

    def test_extension(self) -> None:
        """Test extension method."""
        assert ArchiveType.Conda.extension() == ".conda"
        assert ArchiveType.TarBz2.extension() == ".tar.bz2"

    def test_values(self) -> None:
        """Test enum values."""
        assert ArchiveType.TarBz2.value == 0
        assert ArchiveType.Conda.value == 1


class TestErrorHandling:
    """Tests for error handling."""

    def test_invalid_package_name(self, tmp_path: Path) -> None:
        """Test that invalid package name raises error."""
        files_dir = tmp_path / "files"
        files_dir.mkdir()
        (files_dir / "x.txt").write_text("x")

        with pytest.raises(RattlerBuildError):
            assemble_package(
                name="Invalid Name With Spaces",
                version="1.0.0",
                target_platform="linux-64",
                build_string="0",
                output_dir=tmp_path,
                files_dir=files_dir,
            )

    def test_invalid_platform(self, tmp_path: Path) -> None:
        """Test that invalid platform raises error."""
        files_dir = tmp_path / "files"
        files_dir.mkdir()
        (files_dir / "x.txt").write_text("x")

        with pytest.raises(RattlerBuildError):
            assemble_package(
                name="pkg",
                version="1.0.0",
                target_platform="not-a-platform",
                build_string="0",
                output_dir=tmp_path,
                files_dir=files_dir,
            )
