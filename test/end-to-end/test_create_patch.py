import json
from pathlib import Path


from helpers import RattlerBuild, setup_patch_test_environment, write_simple_text_patch


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


def test_create_patch_always_prints_colored_diff(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Ensures that create-patch prints a colored diff even when not using --dry-run."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_always_color_output",
        cache_files={"test.txt": "hello\n"},
        work_files={"test.txt": "hello world\n"},
    )

    # Run create-patch normally (without --dry-run)
    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        "--overwrite",
    )
    # Should succeed and write patch file
    assert result.returncode == 0
    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    # The colored diff should appear in stderr
    stderr = result.stderr
    # Check for ANSI escape code indicating color output
    assert "\x1b[" in stderr
    # Ensure diff headers and content are present in logs
    assert "a/test.txt" in stderr
    assert "b/test.txt" in stderr
    assert "+hello world" in stderr


def test_create_patch_already_exists_no_overwrite(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Tests that when a patch file already exists and --overwrite is not specified, a message is shown."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_patch_already_exists",
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

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "changes",
        # Note: no --overwrite flag
    )

    # Should succeed (not fail)
    assert result.returncode == 0

    # Should contain the message about not writing the patch file
    stderr = result.stderr
    assert "Not writing patch file, already exists" in stderr
    assert str(patch_path) in stderr


def test_create_patch_incremental_with_existing(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """If a file has already been changed by an existing patch, the new patch should only
    include *new* changes beyond that, not duplicate the original ones."""

    # Initial cache content is `hello`.
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_incremental",
        cache_files={"test.txt": "hello\n"},
        work_files={"test.txt": "hello universe\n"},
    )

    # Write an existing patch that modifies hello -> hello world.
    existing_patch_name = "initial.patch"
    write_simple_text_patch(paths["recipe_dir"], existing_patch_name)

    si_path = paths["work_dir"] / ".source_info.json"
    source_info = json.loads(si_path.read_text())
    source_info["sources"][0]["patches"] = [existing_patch_name]
    si_path.write_text(json.dumps(source_info))

    # Run create-patch to generate a new incremental patch capturing the change from
    # `hello world` -> `hello universe`.
    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--name",
        "incremental",
        "--overwrite",
    )

    assert result.returncode == 0

    new_patch = paths["recipe_dir"] / "incremental.patch"
    assert new_patch.exists()

    content = new_patch.read_text()

    # The new patch should contain the word `universe` (incremental update)
    assert "+hello universe" in content or "+universe" in content

    # It should contain the *removal* of the old line but must not *re-add* it
    assert "-hello world" in content
    assert "+hello world" not in content

    # It should be a proper unified diff containing the expected headers for the modified file
    assert "--- a/test.txt" in content
    assert "+++ b/test.txt" in content
