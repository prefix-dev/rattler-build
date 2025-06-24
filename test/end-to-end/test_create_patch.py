from pathlib import Path


from helpers import RattlerBuild, setup_patch_test_environment


def test_create_patch_modified_file(rattler_build: RattlerBuild, tmp_path: Path):
    """Ensures that modifications to an existing file are picked up and written to the patch."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch",
        cache_files={"test.txt": "hello\n"},
        work_files={"test.txt": "hello world\n"},
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    patch_content = patch_path.read_text()
    assert "+hello world" in patch_content
    assert "a/test.txt" in patch_content
    assert "b/test.txt" in patch_content


def test_create_patch_new_file(rattler_build: RattlerBuild, tmp_path: Path):
    """Verifies that a brand-new file added in the work directory is represented in the generated patch."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_new_file",
        cache_files={},  # No files in cache
        work_files={"added.txt": "brand new file\n"},
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    patch_content = patch_path.read_text()
    assert "b/added.txt" in patch_content
    assert "brand new file" in patch_content


def test_create_patch_deleted_file(rattler_build: RattlerBuild, tmp_path: Path):
    """Confirms that deletions (a file present in the original cache but missing in the work directory) are recorded."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_deleted_file",
        cache_files={"obsolete.txt": "to be deleted\n"},
        work_files={},  # File not present in work directory
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    patch_content = patch_path.read_text()
    assert "a/obsolete.txt" in patch_content
    assert "/dev/null" in patch_content


def test_create_patch_no_changes(rattler_build: RattlerBuild, tmp_path: Path):
    """Checks the no-op scenario: when there are no changes, no patch file should be created."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_no_changes",
        cache_files={"same.txt": "identical\n"},
        work_files={"same.txt": "identical\n"},
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    # No changes -> patch file should NOT exist
    assert not patch_path.exists()


def test_create_patch_custom_output_dir(rattler_build: RattlerBuild, tmp_path: Path):
    """Ensures that `--patch-dir` places the patch file into the requested directory rather than the default."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_custom_output_dir",
        cache_files={"foo.txt": "foo\n"},
        work_files={"foo.txt": "foo bar\n"},
    )

    out_dir = tmp_path / "test_create_patch_custom_output_dir" / "patches"

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--patch-dir",
        str(out_dir),
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = out_dir / "changes.patch"
    assert patch_path.exists()


def test_create_patch_exclude(rattler_build: RattlerBuild, tmp_path: Path):
    """Tests that files passed via `--exclude` are not included in the generated diff."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_exclude",
        cache_files={"ignored.txt": "ignore me\n", "included.txt": "include me\n"},
        work_files={
            "ignored.txt": "ignore me changed\n",
            "included.txt": "include me changed\n",
        },
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--exclude",
        "ignored.txt",
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    content = patch_path.read_text()
    # Ensure diff contains included.txt change but not ignored.txt
    assert "included.txt" in content
    assert "ignored.txt" not in content


def test_create_patch_dry_run(rattler_build: RattlerBuild, tmp_path: Path):
    """Confirms that `--dry-run` prevents writing the patch file even when diffs are detected."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_dry_run",
        cache_files={"file.txt": "hello\n"},
        work_files={"file.txt": "hello world\n"},
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--dry-run",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    # Dry-run should not create the patch
    assert not patch_path.exists()
