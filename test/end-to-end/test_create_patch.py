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
        "--add",
        "added.txt",
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


def test_create_patch_add_pattern(rattler_build: RattlerBuild, tmp_path: Path):
    """Tests that --add patterns control which NEW files are included in the patch."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_add",
        cache_files={},  # No files in cache - both files are new
        work_files={
            "allowed.txt": "this should be added\n",
            "skipped.txt": "this should not be added\n",
        },
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--add",
        "allowed.txt",  # Only add files matching this pattern
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    content = patch_path.read_text()
    # Ensure only the new file matching --add pattern is included
    assert "allowed.txt" in content
    assert "this should be added" in content
    assert "skipped.txt" not in content
    assert "this should not be added" not in content


def test_create_patch_include_pattern(rattler_build: RattlerBuild, tmp_path: Path):
    """Tests that --include patterns control which MODIFIED files are included in the patch."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_include",
        cache_files={
            "included.txt": "original included\n",
            "excluded.txt": "original excluded\n",
        },
        work_files={
            "included.txt": "modified included\n",
            "excluded.txt": "modified excluded\n",
        },
    )

    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--include",
        "included.txt",  # Only include modified files matching this pattern
        "--name",
        "changes",
        "--overwrite",
    )

    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()

    content = patch_path.read_text()
    # Ensure only the modified file matching --include pattern is in the patch
    assert "included.txt" in content
    assert "modified included" in content
    assert "excluded.txt" not in content
    assert "modified excluded" not in content


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
        # "--include",
        # "*",
        "--name",
        "changes",
        "--dry-run",
    )

    print(result.stdout)
    print(result.stderr)
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

    # Should warn about existing patch file without overwrite
    stderr = result.stderr
    # Matches the warning logged when patch exists and no --overwrite is given
    assert "Use --overwrite to replace the existing patch file" in stderr


def test_create_patch_incremental_with_existing(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """If a file has already been changed by an existing patch, the new patch should only
    include *new* changes beyond that, not duplicate the original ones."""

    # Initial cache and work; include existing patch in source info
    existing_patch_name = "initial.patch"
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_incremental",
        cache_files={"test.txt": "hello\n"},
        work_files={"test.txt": "hello universe\n"},
        patches=[existing_patch_name],
    )

    # Write an existing patch that modifies hello -> hello world
    write_simple_text_patch(paths["recipe_dir"], existing_patch_name)

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


def test_create_patch_nested_subdirectories(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Ensures nested subdirectory files are diffed correctly."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_nested",
        cache_files={},
        work_files={},
    )
    orig_dir = paths["cache_dir"] / "example_01234567"
    work_dir = paths["work_dir"]
    nested_cache = orig_dir / "dir" / "nested"
    nested_cache.mkdir(parents=True)
    (nested_cache / "file.txt").write_text("hello\n")
    nested_work = work_dir / "dir" / "nested"
    nested_work.mkdir(parents=True)
    (nested_work / "file.txt").write_text("hello universe\n")
    result = rattler_build(
        "create-patch",
        "--directory",
        str(work_dir),
        "--name",
        "changes",
        "--overwrite",
    )
    assert result.returncode == 0
    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()
    content = patch_path.read_text()
    assert "a/dir/nested/file.txt" in content
    assert "b/dir/nested/file.txt" in content
    assert "+hello universe" in content


def test_create_patch_skips_binary_files(rattler_build: RattlerBuild, tmp_path: Path):
    """Ensures that binary files are skipped and do not cause errors or appear in the patch."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_skips_binary",
        cache_files={"text.txt": "hello\n"},
        work_files={"text.txt": "hello world\n"},
    )

    orig_dir = paths["cache_dir"] / "example_01234567"
    work_dir = paths["work_dir"]
    binary_cache = orig_dir / "binary.bin"
    binary_cache.write_bytes(b"\x00\xff\x00\xff")
    binary_work = work_dir / "binary.bin"
    binary_work.write_bytes(b"\x00\xff\x00\xfa")

    result = rattler_build(
        "create-patch",
        "--directory",
        str(work_dir),
        "--name",
        "changes",
        "--overwrite",
    )
    assert result.returncode == 0

    # Check patch file
    patch_path = paths["recipe_dir"] / "changes.patch"
    assert patch_path.exists()
    content = patch_path.read_text()
    # Ensure only text diff is included
    assert "text.txt" in content
    assert "binary.bin" not in content

    # Ensure skip warning is logged
    stderr = result.stderr
    assert "Skipping binary file" in stderr


def test_create_patch_binary_file_deletion(rattler_build: RattlerBuild, tmp_path: Path):
    """Ensures that deleting a binary file logs the skip and emits a deletion header to /dev/null."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_create_patch_binary_deletion",
        cache_files={},
        work_files={},
    )

    orig_dir = paths["cache_dir"] / "example_01234567"
    work_dir = paths["work_dir"]
    deleted_bin = orig_dir / "binary_delete.bin"
    deleted_bin.write_bytes(b"\x00\xff\x00\xff")

    result = rattler_build(
        "create-patch",
        "--directory",
        str(work_dir),
        "--name",
        "delete-bin",
        "--overwrite",
    )
    assert result.returncode == 0

    patch_path = paths["recipe_dir"] / "delete-bin.patch"
    assert patch_path.exists()
    content = patch_path.read_text()
    # Deletion header should reference the binary file and /dev/null
    assert "--- a/binary_delete.bin" in content
    assert "+++ /dev/null" in content

    stderr = result.stderr
    # Should warn about skipping binary file deletion
    assert "Skipping binary file deletion" in stderr


def test_create_patch_incremental_map_strategy(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Ensures that only the relevant existing patches are applied per-file when generating an incremental patch."""
    paths = setup_patch_test_environment(
        tmp_path,
        "test_incremental_map",
        cache_files={"a.txt": "alpha\n", "b.txt": "beta\n"},
        work_files={"a.txt": "alpha1\n", "b.txt": "beta1\n"},
        patches=["initial_a.patch", "initial_b.patch"],
    )
    # Two existing patches: one for a.txt, one for b.txt
    write_simple_text_patch(
        paths["recipe_dir"],
        "initial_a.patch",
        old="alpha",
        new="alpha1",
        target_file="a.txt",
    )
    write_simple_text_patch(
        paths["recipe_dir"],
        "initial_b.patch",
        old="beta",
        new="beta1",
        target_file="b.txt",
    )

    # Further modify only a.txt in work
    (paths["work_dir"] / "a.txt").write_text("alpha2\n")

    # Generate patch
    result = rattler_build(
        "create-patch",
        "--directory",
        str(paths["work_dir"]),
        "--overwrite",
    )
    assert result.returncode == 0

    new_patch = paths["recipe_dir"] / "changes.patch"
    assert new_patch.exists()
    content = new_patch.read_text()

    # Ensure b.txt is not re-patched (unchanged)
    assert "a/b.txt" not in content
    assert "b/b.txt" not in content

    # Check that a.txt changes are present, comparing from the previous patch baseline
    assert "--- a/a.txt" in content
    assert "+++ b/a.txt" in content
    assert "-alpha1" in content
    assert "+alpha2" in content
