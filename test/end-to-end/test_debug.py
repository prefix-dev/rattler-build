import platform
from pathlib import Path
from subprocess import CalledProcessError

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

    _out, err = capfd.readouterr()
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
    with pytest.raises(CalledProcessError):
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
        with open(work_dir / "conda_build_script.bat") as f:
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
        with open(work_dir / "conda_build_script.bat") as f:
            content = f.read()
            assert "Building output2" in content
    else:
        assert (work_dir / "conda_build.sh").exists()
        with open(work_dir / "conda_build.sh") as f:
            content = f.read()
            assert "Building output2" in content

    # should fail with invalid output name
    with pytest.raises(CalledProcessError):
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


def test_debug_run(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build(
        "debug",
        "setup",
        "--recipe",
        str(recipes / "debug_run"),
        "--output-dir",
        str(tmp_path),
    )
    work_dir = next(tmp_path.glob("**/work"))
    if platform.system() == "Windows":
        host_prefix = next(tmp_path.glob("**/h_env"))
    else:
        host_prefix = next(tmp_path.glob("**/host_env_placehold*"))

    # The script runs in the work directory with the build environment set up,
    # whatever the current directory of the caller is.
    output = rattler_build("debug", "run", "--work-dir", str(work_dir))
    script_cwd = next(
        line.removeprefix("running in ").strip()
        for line in output.splitlines()
        if line.startswith("running in ")
    )
    assert Path(script_cwd).resolve() == work_dir.resolve()
    assert (host_prefix / "hello.txt").read_text().strip() == "hello from debug run"

    # The exit code of the script is the exit code of `debug run`.
    (host_prefix / "fail").touch()
    with pytest.raises(CalledProcessError) as failure:
        rattler_build("debug", "run", "--work-dir", str(work_dir))
    assert failure.value.returncode == 3
