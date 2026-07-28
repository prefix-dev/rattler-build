from pathlib import Path
import platform
import pytest
from helpers import RattlerBuild


def test_debug_basic(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, capfd):
    rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug_test"),
        "--output-dir",
        str(tmp_path),
    )

    out, err = capfd.readouterr()
    assert "Build and/or host environments created for debugging" in err
    assert "To run the actual build, use:" in err
    assert "rattler-build build --recipe" in err

    # work directory check to see if it was created
    work_dir = next(tmp_path.glob("**/work"))
    assert work_dir.exists()

    # checking for build scripts to see if they were created
    if platform.system() == "Windows":
        assert (work_dir / "conda_build.bat").exists()
    else:
        assert (work_dir / "conda_build.sh").exists()


def test_debug_multiple_outputs(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, capfd
):
    # should fail without --output-name
    with pytest.raises(Exception):
        rattler_build(
            "debug",
            "setup",
            "--recipe",
            str(recipes / "debug-multiple-outputs"),
            "--output-dir",
            str(tmp_path),
        )

    # should work with --output-name for output1
    rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug-multiple-outputs"),
        "--output-dir",
        str(tmp_path),
        "--output-name",
        "output1",
    )

    # work directory check to see if it was created
    work_dir = next(tmp_path.glob("**/work"))
    assert work_dir.exists()

    # checking for build scripts to see if they were created
    if platform.system() == "Windows":
        assert (work_dir / "conda_build.bat").exists()
        with open(work_dir / "conda_build.bat") as f:
            content = f.read()
            assert "Building output1" in content
    else:
        assert (work_dir / "conda_build.sh").exists()
        with open(work_dir / "conda_build.sh") as f:
            content = f.read()
            assert "Building output1" in content

    # clean up work directory for next test
    import shutil

    shutil.rmtree(work_dir)

    # test output2 as well
    rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug-multiple-outputs"),
        "--output-dir",
        str(tmp_path),
        "--output-name",
        "output2",
    )

    work_dir = next(tmp_path.glob("**/work"))
    assert work_dir.exists()

    if platform.system() == "Windows":
        assert (work_dir / "conda_build.bat").exists()
        with open(work_dir / "conda_build.bat") as f:
            content = f.read()
            assert "Building output2" in content
    else:
        assert (work_dir / "conda_build.sh").exists()
        with open(work_dir / "conda_build.sh") as f:
            content = f.read()
            assert "Building output2" in content

    # should fail with invalid output name
    with pytest.raises(Exception):
        rattler_build(
            "debug",
            "setup",
            "--recipe",
            str(recipes / "debug-multiple-outputs"),
            "--output-dir",
            str(tmp_path),
            "--output-name",
            "invalid_output",
        )


def _build_script(work_dir: Path) -> str:
    name = "conda_build.bat" if platform.system() == "Windows" else "conda_build.sh"
    script = work_dir / name
    assert script.exists()
    return script.read_text(encoding="utf-8")


def test_debug_staging_output(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    # The staging (compile) output is selectable by name and its debug
    # environment runs the staging build script.
    stage_dir = tmp_path / "stage"
    rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug-staging"),
        "--output-dir",
        str(stage_dir),
        "--output-name",
        "build-stage",
    )
    work_dir = next(stage_dir.glob("**/work"))
    assert "Compiling in staging" in _build_script(work_dir)

    # An inheriting package output builds/restores the staging cache, so its
    # debug work dir is populated with the staged ./install tree.
    pkg_dir = tmp_path / "pkg"
    rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug-staging"),
        "--output-dir",
        str(pkg_dir),
        "--output-name",
        "debug-staging",
    )
    work_dir = next(pkg_dir.glob("**/work"))
    assert "Packaging from staging" in _build_script(work_dir)
    assert (work_dir / "install" / "artifact.txt").exists()


def test_debug_staging_listed_in_available_outputs(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    # An unknown --output-name reports both the package output and the staging
    # (compile) output as valid targets.
    result = rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug-staging"),
        "--output-dir",
        str(tmp_path),
        "--output-name",
        "does-not-exist",
        need_result_object=True,
        # rattler-build emits UTF-8 (incl. the box-drawing error frame); decode
        # it explicitly so the Windows locale (cp1252) does not choke.
        capture_output=True,
        encoding="utf-8",
        errors="replace",
    )
    assert result.returncode != 0
    combined = (result.stdout or "") + (result.stderr or "")
    assert "Available outputs" in combined
    assert "build-stage" in combined
    assert "debug-staging" in combined
