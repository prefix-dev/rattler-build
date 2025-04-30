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
