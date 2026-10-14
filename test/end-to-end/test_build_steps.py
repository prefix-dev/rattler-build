import shutil
from pathlib import Path

import yaml
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


def test_default_build_script_still_runs(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A legacy build.sh/build.bat is still discovered when no script is declared."""
    rattler_build.build(recipes / "default_build_script", tmp_path)
    pkg = get_extracted_package(tmp_path, "default_build_script_test")

    marker = pkg / "share" / "default_build_script" / "marker.txt"
    assert marker.exists(), "default build script did not run"
    assert "default-build-script" in marker.read_text()


def test_packaged_step_provider_uses_standalone_environment(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, monkeypatch
):
    """Packaged actions compile recursively before variants and rebuild source-free."""
    cache = tmp_path / "cache"
    monkeypatch.setenv("RATTLER_CACHE_DIR", str(cache))
    provider_recipe = tmp_path / "provider-recipe"
    consumer_recipe = tmp_path / "consumer-recipe"
    shutil.copytree(recipes / "step_provider", provider_recipe)
    shutil.copytree(recipes / "step_provider_consumer", consumer_recipe)
    provider_output = tmp_path / "provider-output"
    channel = tmp_path / "channel"
    consumer_output = tmp_path / "consumer-output"
    rattler_build.build(provider_recipe, provider_output)
    provider_package = get_package(provider_output, "test-rattler-build-steps")
    rattler_build("publish", str(provider_package), "--to", str(channel))

    variants = tmp_path / "variants.yaml"
    variants.write_text('python: ["3.11", "3.12"]\n')
    rendered = rattler_build.render(
        consumer_recipe,
        consumer_output,
        with_solve=True,
        variant_config=variants,
        custom_channels=[channel.as_uri(), "conda-forge"],
        extra_args=["--experimental"],
    )
    assert {
        output["build_configuration"]["variant"]["python"] for output in rendered
    } == {"3.11", "3.12"}
    for output in rendered:
        assert {
            ".".join(record["version"].split(".")[:2])
            for record in output["finalized_dependencies"]["build"]["resolved"]
            if record["name"] == "python"
        } == {output["build_configuration"]["variant"]["python"]}

    rattler_build.build(
        consumer_recipe,
        consumer_output,
        custom_channels=[channel.as_uri(), "conda-forge"],
        extra_args=["--experimental"],
    )
    pkg = get_extracted_package(consumer_output, "step-provider-consumer")
    assert (
        pkg / "share" / "step-provider" / "marker.txt"
    ).read_text().strip() == "exact-provider-worked"
    assert not any(pkg.rglob("test-rattler-build-steps*"))
    assert (pkg / "share/step-provider/typed.txt").read_text() == "true:8:first,second"
    assert (
        pkg / "share/step-provider/nested.txt"
    ).read_text() == "exact-provider-worked"
    stored = yaml.safe_load((pkg / "info/recipe/rendered_recipe.yaml").read_text())
    assert not {"six", "test-rattler-build-steps"}.intersection(
        record["name"]
        for record in stored["finalized_dependencies"]["build"]["resolved"]
    )
    assert all("uses" not in step for step in stored["recipe"]["build"]["steps"])
    assert str(cache) not in yaml.safe_dump(stored["recipe"]["build"])

    # Remove both installed documents and their channel: a rebuild must execute
    # the embedded flat plan, not silently retrieve the provider a second time.
    shutil.rmtree(provider_recipe)
    shutil.rmtree(consumer_recipe)
    shutil.rmtree(channel)
    shutil.rmtree(cache)
    rebuilt_output = tmp_path / "rebuilt-output"
    rattler_build(
        "rebuild",
        "--package-file",
        str(get_package(consumer_output, "step-provider-consumer")),
        "--output-dir",
        str(rebuilt_output),
        "--experimental",
    )
    rebuilt = get_extracted_package(rebuilt_output, "step-provider-consumer")
    assert (
        rebuilt / "share/step-provider/typed.txt"
    ).read_text() == "true:8:first,second"
    assert (
        rebuilt / "share/step-provider/nested.txt"
    ).read_text() == "exact-provider-worked"
