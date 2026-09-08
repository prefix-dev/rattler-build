import json
import shutil
from pathlib import Path
from subprocess import STDOUT

import pytest
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


def test_metadata_step_runs_before_solving_and_defines_build_plan(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Metadata bootstrap output participates in solving and build execution."""
    rattler_build.build(
        recipes / "metadata_step", tmp_path, extra_args=["--experimental"]
    )
    pkg = get_extracted_package(tmp_path, "metadata-step-example")

    assert (
        pkg / "share" / "metadata-step-example" / "generated.txt"
    ).read_text() == "overridden by recipe\n"
    run_exports = json.loads((pkg / "info" / "run_exports.json").read_text())
    assert run_exports["weak"] == ["metadata-abi"]


def test_metadata_dependencies_expand_variants_after_generation(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A dependency introduced by metadata participates in the final matrix."""
    variant_config = tmp_path / "variants.yaml"
    variant_config.write_text("zlib:\n  - 1.2\n  - 1.3\n")

    rendered = rattler_build.render(
        recipes / "metadata_step",
        tmp_path / "output",
        variant_config=variant_config,
        extra_args=["--experimental"],
    )

    assert len(rendered) == 2
    assert {
        output["build_configuration"]["variant"]["zlib"] for output in rendered
    } == {"1.2", "1.3"}
    assert all(
        output["recipe"]["build"]["steps"][0]["name"] == "install"
        for output in rendered
    )


def test_generated_provider_requirements_expand_metadata_variants(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Requirements hidden in a generated provider are included in the matrix."""
    recipe_dir = tmp_path / "generated-provider-variant"
    recipe_dir.mkdir()
    (recipe_dir / "provider.yaml").write_text(
        """requirements:
  build: [zlib]
steps:
  - name: compile
    run: echo compiled
"""
    )
    (recipe_dir / "recipe.yaml").write_text(
        """schema_version: 1
package:
  name: generated-provider-variant
  version: 1.0.0
build:
  metadata:
    requirements:
      build: [python]
    interpreter: python
    run: |
      import json
      import os
      with open(os.environ["OUTPUT_FILE"], "w") as output:
          output.write("build.steps " + json.dumps([{"name": "compile", "uses": "./provider.yaml"}]) + "\\n")
          output.write('build.variant.use_keys.append ["libpng"]\\n')
"""
    )
    variant_config = tmp_path / "provider-variants.yaml"
    variant_config.write_text(
        """zlib:
  - 1.2
  - 1.3
libpng:
  - 1.6.42
  - 1.6.43
zip_keys:
  - [zlib, libpng]
"""
    )

    rendered = rattler_build.render(
        recipe_dir,
        tmp_path / "output",
        variant_config=variant_config,
        extra_args=["--experimental"],
    )

    assert len(rendered) == 2
    assert {
        (
            output["build_configuration"]["variant"]["zlib"],
            output["build_configuration"]["variant"]["libpng"],
        )
        for output in rendered
    } == {("1.2", "1.6.42"), ("1.3", "1.6.43")}


@pytest.mark.parametrize("recipe_name", ["", "  name: metadata-multi-output\n"])
def test_metadata_rejects_multi_output_graphs_before_execution(
    rattler_build: RattlerBuild, tmp_path: Path, recipe_name: str
):
    """Even metadata without new variants would invalidate downstream exact-pin hashes."""
    recipe_dir = tmp_path / "metadata-multi-output"
    recipe_dir.mkdir()
    (recipe_dir / "recipe.yaml").write_text(
        "schema_version: 1\nrecipe:\n"
        + recipe_name
        + """  version: 1.0.0
outputs:
  - package:
      name: upstream
    build:
      metadata:
        interpreter: python
        run: |
          raise AssertionError("metadata must not execute")
  - package:
      name: downstream
    requirements:
      run:
        - ${{ pin_subpackage("upstream", exact=True) }}
"""
    )

    result = rattler_build(
        "build",
        "--recipe",
        str(recipe_dir),
        "--output-dir",
        str(tmp_path / "output"),
        "--render-only",
        "--experimental",
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert (
        "build.metadata in multi-output recipes is not yet supported" in result.stderr
    )
    assert "hashes must be recomputed afterward" in result.stderr
    assert "Running pre-solve metadata step" not in result.stderr


def test_metadata_requires_output_file(rattler_build: RattlerBuild, tmp_path: Path):
    """A successful command that forgets the metadata protocol is an error."""
    recipe = tmp_path / "missing-output" / "recipe.yaml"
    recipe.parent.mkdir()
    recipe.write_text(
        """schema_version: 1
package:
  name: missing-metadata-output
  version: 1.0.0
build:
  metadata:
    run: echo metadata command ran
"""
    )
    result = rattler_build(
        "build",
        "--recipe",
        str(recipe),
        "--output-dir",
        str(tmp_path / "output"),
        "--render-only",
        "--experimental",
        capture_output=True,
    )

    assert result.returncode != 0
    assert "completed without creating OUTPUT_FILE" in result.stderr


def test_metadata_generated_steps_require_names(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Generated defaults are always addressable for recipe overrides."""
    recipe = tmp_path / "unnamed-step" / "recipe.yaml"
    recipe.parent.mkdir()
    recipe.write_text(
        """schema_version: 1
package:
  name: unnamed-generated-step
  version: 1.0.0
build:
  metadata:
    requirements:
      build: [python]
    interpreter: python
    run: |
      import os
      with open(os.environ["OUTPUT_FILE"], "w") as output:
          output.write('build.steps [{"run":"echo generated"}]\\n')
"""
    )
    result = rattler_build(
        "build",
        "--recipe",
        str(recipe),
        "--output-dir",
        str(tmp_path / "output"),
        "--render-only",
        "--experimental",
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "generated an unnamed build step" in result.stderr


def test_run_metadata_uses_external_source_tree(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """`run --source-dir` is visible to metadata, not only generated steps."""
    source = tmp_path / "external-source"
    source.mkdir()
    (source / "pyproject.toml").write_text(
        '[tool.rattler-build]\nbuild = ["python"]\nhost = []\n'
    )
    output = rattler_build(
        "run",
        "install",
        "--recipe",
        str(recipes / "metadata_step"),
        "--source-dir",
        str(source),
        "--output-dir",
        str(tmp_path / "output"),
        "--experimental",
        stderr=STDOUT,
    )

    assert str(source) in output
    assert "- zlib" not in output


def test_python_metadata_backend_builds_external_rich_source(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """One provider package supplies both pyproject metadata and wheel steps."""
    provider_output = tmp_path / "provider-output"
    channel = tmp_path / "channel"
    consumer_output = tmp_path / "consumer-output"
    variant_config = tmp_path / "rich-variants.yaml"
    variant_config.write_text("python:\n  - 3.11\n  - 3.12\n")
    rattler_build.build(recipes / "metadata_python_provider", provider_output)
    provider = get_package(provider_output, "python-rattler-build-steps")
    rattler_build("publish", str(provider), "--to", str(channel))
    build_args = rattler_build.build_args(
        recipes / "metadata_python_backend",
        consumer_output,
        variant_config=variant_config,
        custom_channels=[channel.as_uri(), "conda-forge"],
        extra_args=["--experimental"],
    )
    rattler_build(*build_args, stderr=STDOUT)
    pkg = get_extracted_package(consumer_output, "rich")

    index = json.loads((pkg / "info" / "index.json").read_text())
    assert index["noarch"] == "python"
    assert "python >=3.8.0" in index["depends"]
    assert "markdown-it-py >=2.2.0" in index["depends"]
    assert "pygments >=2.13.0,<3" in index["depends"]
    about = json.loads((pkg / "info" / "about.json").read_text())
    assert about["license"] == "MIT"
    assert about["summary"].startswith("Render rich text")
    rendered = yaml.safe_load(
        (pkg / "info" / "recipe" / "rendered_recipe.yaml").read_text()
    )["recipe"]
    assert {"python", "pip", "python-build", "poetry-core >=1.0.0"} <= set(
        rendered["requirements"]["host"]
    )
    assert (pkg / "site-packages" / "rich" / "__init__.py").exists()
    assert (pkg / "info" / "licenses" / "LICENSE").exists()


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

    index = json.loads((pkg / "info" / "index.json").read_text())
    assert "zlib" in index["depends"]
    run_exports = json.loads((pkg / "info" / "run_exports.json").read_text())
    assert run_exports["strong"] == ["reusable-abi"]
    about = json.loads((pkg / "info" / "about.json").read_text())
    assert about["dev_url"] == "https://example.com/reusable-step"


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
    (metadata,) = output.glob("bld/*/work/.rattler-build/step-outputs/*.txt")
    assert metadata.read_text() == "about.summary cached metadata\n"
    assert not (project / ".rattler-build").exists()
    rattler_build(*args)
    assert metadata.read_text() == "about.summary cached metadata\n"
    assert (project / "run-count.txt").read_text().splitlines() == ["run"]

    (project / "input.txt").write_text("changed\n")
    rattler_build(*args)
    assert (project / "run-count.txt").read_text().splitlines() == ["run", "run"]
    assert (project / "generated.txt").read_text() == "changed\n"
    assert not metadata.exists(), "a cache miss must discard stale metadata"
    metadata.write_text("about.summary stale metadata\n")
    rattler_build(*args)
    assert not metadata.exists(), "a cache hit must replay intentional metadata absence"
    assert (project / "run-count.txt").read_text().splitlines() == ["run", "run"]


def test_step_cache_failed_miss_cannot_revive_success(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    project = tmp_path / "project"
    shutil.copytree(recipes / "step_cache", project)
    recipe_path = project / "recipe.yaml"
    recipe = yaml.safe_load(recipe_path.read_text())
    recipe["build"]["steps"][0]["run"] += (
        '\nif Path("fail").exists():\n    raise SystemExit(42)\n'
    )
    recipe_path.write_text(yaml.safe_dump(recipe))
    args = (
        "run",
        "cached",
        "--recipe",
        str(project),
        "--source-dir",
        str(project),
        "--output-dir",
        str(tmp_path / "output"),
        "--experimental",
    )
    rattler_build(*args)
    # Force a miss, then recreate exactly the successful output/declarations
    # before failing. A surviving old state would incorrectly skip the retry.
    (project / "generated.txt").unlink()
    (project / "fail").touch()
    assert rattler_build(*args, need_result_object=True).returncode != 0
    assert rattler_build(*args, need_result_object=True).returncode != 0
    assert (project / "run-count.txt").read_text().splitlines() == ["run"] * 3
    (project / "fail").unlink()
    rattler_build(*args)
    rattler_build(*args)
    assert (project / "run-count.txt").read_text().splitlines() == ["run"] * 4


@pytest.mark.parametrize("payload_change", [None, "delete", "alter"])
def test_step_cache_replays_package_metadata(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, payload_change
):
    """Repeated ordinary builds recreate work/, but preserve verified metadata."""
    project = tmp_path / "project"
    shutil.copytree(recipes / "step_cache", project)
    recipe_path = project / "recipe.yaml"
    recipe = yaml.safe_load(recipe_path.read_text())
    step = recipe["build"]["steps"][0]
    step["cwd"] = str(project)
    step["run"] += (
        '\nwith Path(os.environ["OUTPUT_FILE"]).open("a") as output:\n'
        '    output.write("build.prefix_detection.ignore_binary_files true\\n")\n'
    )
    recipe_path.write_text(yaml.safe_dump(recipe))
    output = tmp_path / "output"
    args = rattler_build.build_args(
        project,
        output,
        extra_args=["--experimental", "--no-build-id", "--keep-build"],
    )
    rattler_build(*args)
    pkg = get_extracted_package(output, "step-cache-test")
    assert (
        json.loads((pkg / "info/about.json").read_text())["summary"]
        == "cached metadata"
    )
    shutil.rmtree(output / "extract")
    # A mutable work copy must never be authoritative on a cache hit.
    (metadata,) = output.glob("bld/*/work/.rattler-build/step-outputs/*.txt")
    metadata.write_text("about.summary corrupted work copy\n")
    if payload_change is not None:
        (payload,) = output.glob("bld/*/.rattler-build-step-cache/*.output")
        if payload_change == "delete":
            payload.unlink()
        else:
            payload.write_text("about.summary corrupted replay payload\n")
    rattler_build(*args)
    pkg = get_extracted_package(output, "step-cache-test")
    assert (
        json.loads((pkg / "info/about.json").read_text())["summary"]
        == "cached metadata"
    )
    expected_runs = 1 if payload_change is None else 2
    assert (project / "run-count.txt").read_text().splitlines() == [
        "run"
    ] * expected_runs


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


def test_rebuild_applies_post_build_outputs_once(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = tmp_path / "project"
    project.mkdir()
    (project / "recipe.yaml").write_text(
        yaml.safe_dump(
            {
                "package": {"name": "rebuild-outputs", "version": "1"},
                "requirements": {"build": ["python"]},
                "build": {
                    "steps": [
                        {
                            "interpreter": "python",
                            "run": (
                                "import json\n"
                                "import os\n"
                                "from pathlib import Path\n"
                                'Path(os.environ["PREFIX"], "marker.txt").write_text("built")\n'
                                'rules = [{"files": ["**/marker.txt"], "regex": "$", "replacement": "!"}] * 2\n'
                                'Path(os.environ["OUTPUT_FILE"]).write_text("build.post_process.append " + json.dumps(rules) + "\\n")'
                            ),
                        }
                    ]
                },
            }
        )
    )
    output = tmp_path / "original"
    rattler_build.build(project, output, extra_args=["--experimental"])
    assert (
        get_extracted_package(output, "rebuild-outputs") / "marker.txt"
    ).read_text() == "built!!"
    package = get_package(output, "rebuild-outputs")
    for index in range(2):
        output = tmp_path / f"rebuilt-{index}"
        rattler_build(
            "rebuild",
            "--package-file",
            str(package),
            "--output-dir",
            str(output),
            "--experimental",
        )
        assert (
            get_extracted_package(output, "rebuild-outputs") / "marker.txt"
        ).read_text() == "built!!"
        package = get_package(output, "rebuild-outputs")
def test_metadata_run_preserves_prepared_sources_and_cached_outputs(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    project = tmp_path / "project"
    shutil.copytree(recipes / "step_cache", project)
    recipe_path = project / "recipe.yaml"
    recipe = yaml.safe_load(recipe_path.read_text())
    recipe["source"] = {"path": "input.txt"}
    recipe["build"]["metadata"] = {
        "requirements": {"build": ["python"]},
        "interpreter": "python",
        "run": 'import os\nfrom pathlib import Path\nPath(os.environ["OUTPUT_FILE"]).write_text("about.summary generated metadata\\n")',
    }
    recipe_path.write_text(yaml.safe_dump(recipe))
    output = tmp_path / "output"
    args = (
        "run",
        "cached",
        "--recipe",
        str(project),
        "--output-dir",
        str(output),
        "--experimental",
    )
    rattler_build(*args)
    (work,) = output.glob("bld/*/work")
    (work / "retained-artifact.txt").write_text("keep me")
    rattler_build(*args)
    assert (work / "run-count.txt").read_text() == "run\n"
    assert (work / "retained-artifact.txt").read_text() == "keep me"
    (metadata,) = (work / ".rattler-build/step-outputs").glob("*.txt")
    assert metadata.read_text() == "about.summary cached metadata\n"
