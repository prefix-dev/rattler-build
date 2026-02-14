import os
from pathlib import Path

import pytest
from helpers import RattlerBuild, get_extracted_package


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_perl_tests(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot
):
    rattler_build.build(recipes / "perl-test", tmp_path)
    pkg = get_extracted_package(tmp_path, "perl-call-context")

    assert (pkg / "info" / "tests" / "tests.yaml").exists()
    content = (pkg / "info" / "tests" / "tests.yaml").read_text()

    assert snapshot == content


def test_r_tests(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot):
    rattler_build.build(recipes / "r-test", tmp_path)
    pkg = get_extracted_package(tmp_path, "r-test")

    assert (pkg / "info" / "tests" / "tests.yaml").exists()
    content = (pkg / "info" / "tests" / "tests.yaml").read_text()

    assert snapshot == content


def test_source_files_copied_to_test(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that files.source: ['./'] copies all source files to the test directory (issue #2085)."""
    rattler_build.build(recipes / "test-source-files", tmp_path)
    pkg = get_extracted_package(tmp_path, "test-source-files")

    # Verify the test files were packaged
    assert (pkg / "etc" / "conda" / "test-files" / "test-source-files" / "0" / "data.txt").exists()


@pytest.mark.skipif(
    os.name != "nt", reason="recipe does not support execution on windows"
)
def test_win_errorlevel_injection(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot
):
    rattler_build.build(
        recipes / "test-errorlevel-injection", tmp_path, extra_args=["--test=skip"]
    )
    pkg = get_extracted_package(tmp_path, "test-errorlevel-injection")

    assert (pkg / "info" / "tests" / "tests.yaml").exists()
    content = (pkg / "info" / "tests" / "tests.yaml").read_text()

    assert snapshot == content
