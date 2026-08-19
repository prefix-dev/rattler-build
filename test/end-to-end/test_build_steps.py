import shutil
from pathlib import Path

from helpers import RattlerBuild, get_extracted_package, get_package


def test_build_steps(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """`build.steps` compiles into the generated wrapper and runs in order.

    Run steps execute as scoped sections: one writes via the build-time
    `$PREFIX`, one uses step-local `env`, one proves env does not leak, and one
    runs from a step-local `cwd`.
    """
    rattler_build.build(
        recipes / "build_steps", tmp_path, extra_args=["--experimental"]
    )
    pkg = get_extracted_package(tmp_path, "build_steps_test")

    step1 = pkg / "share" / "build_steps" / "step1.txt"
    step2 = pkg / "share" / "build_steps" / "step2.txt"
    step3 = pkg / "share" / "build_steps" / "step3.txt"
    cwd_pwd = pkg / "share" / "build_steps" / "cwd" / "pwd.txt"

    assert step1.exists(), "first step did not run"
    assert step2.exists(), "second step did not run"
    assert step3.exists(), "third step did not run"
    assert cwd_pwd.exists(), "cwd step did not run in its target directory"
    assert "hello-from-step" in step2.read_text(), (
        "step-local env did not reach the section"
    )
    assert "unset" in step3.read_text(), "step-local env leaked to a later section"


def test_reusable_steps_inputs_and_generated_licenses(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Reusable inputs render before solving and generated licenses are metadata-only."""
    rattler_build.build(
        recipes / "reusable_steps", tmp_path, extra_args=["--experimental"]
    )
    pkg = get_extracted_package(tmp_path, "reusable_steps_test")

    assert (pkg / "share" / "reusable-steps" / "marker.txt").exists()
    license_file = pkg / "info" / "licenses" / "dependency.txt"
    assert license_file.read_text().strip() == "dependency-license"
    assert not (pkg / "generated-licenses").exists()


def test_packaged_step_provider_uses_standalone_environment(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A versioned provider resolves from a local channel without entering the package."""
    provider_output = tmp_path / "provider-output"
    channel = tmp_path / "channel"
    consumer_output = tmp_path / "consumer-output"
    rattler_build.build(recipes / "step_provider", provider_output)
    provider_package = get_package(provider_output, "test-rattler-build-steps")
    rattler_build("publish", str(provider_package), "--to", str(channel))

    rattler_build.build(
        recipes / "step_provider_consumer",
        consumer_output,
        custom_channels=[channel.as_uri(), "conda-forge"],
        extra_args=["--experimental"],
    )
    pkg = get_extracted_package(consumer_output, "step-provider-consumer")
    assert (
        pkg / "share" / "step-provider" / "marker.txt"
    ).read_text().strip() == "exact-provider-worked"
    assert not any(pkg.rglob("test-rattler-build-steps*"))


def test_step_cache_skips_and_invalidates(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Step-written input/output conditions skip work until an input changes."""
    project = tmp_path / "project"
    shutil.copytree(recipes / "step_cache", project)
    output = tmp_path / "output"
    args = (
        "run",
        "cached",
        "--recipe",
        str(project),
        "--source-dir",
        str(project),
        "--output-dir",
        str(output),
        "--experimental",
    )

    rattler_build(*args)
    rattler_build(*args)
    assert (project / "run-count.txt").read_text().splitlines() == ["run"]

    (project / "input.txt").write_text("changed\n")
    rattler_build(*args)
    assert (project / "run-count.txt").read_text().splitlines() == ["run", "run"]
    assert (project / "generated.txt").read_text() == "changed\n"


def test_default_build_script_still_runs(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A legacy build.sh/build.bat is still discovered when no script is declared."""
    rattler_build.build(recipes / "default_build_script", tmp_path)
    pkg = get_extracted_package(tmp_path, "default_build_script_test")

    marker = pkg / "share" / "default_build_script" / "marker.txt"
    assert marker.exists(), "default build script did not run"
    assert "default-build-script" in marker.read_text()
