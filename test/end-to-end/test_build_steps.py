from pathlib import Path

from helpers import RattlerBuild, get_extracted_package


def test_build_steps(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """`build.steps` compiles into the generated wrapper and runs in order.

    Run steps execute as scoped sections: one writes via the build-time
    `$PREFIX`, one uses step-local `env`, one proves env does not leak, and one
    runs from a step-local `cwd`.
    """
    rattler_build.build(recipes / "build_steps", tmp_path, extra_args=["--experimental"])
    pkg = get_extracted_package(tmp_path, "build_steps_test")

    step1 = pkg / "share" / "build_steps" / "step1.txt"
    step2 = pkg / "share" / "build_steps" / "step2.txt"
    step3 = pkg / "share" / "build_steps" / "step3.txt"
    cwd_pwd = pkg / "share" / "build_steps" / "cwd" / "pwd.txt"

    assert step1.exists(), "first step did not run"
    assert step2.exists(), "second step did not run"
    assert step3.exists(), "third step did not run"
    assert cwd_pwd.exists(), "cwd step did not run in its target directory"
    assert "hello-from-step" in step2.read_text(), "step-local env did not reach the section"
    assert "unset" in step3.read_text(), "step-local env leaked to a later section"


def test_default_build_script_still_runs(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A legacy build.sh/build.bat is still discovered when no script is declared."""
    rattler_build.build(recipes / "default_build_script", tmp_path)
    pkg = get_extracted_package(tmp_path, "default_build_script_test")

    marker = pkg / "share" / "default_build_script" / "marker.txt"
    assert marker.exists(), "default build script did not run"
    assert "default-build-script" in marker.read_text()
