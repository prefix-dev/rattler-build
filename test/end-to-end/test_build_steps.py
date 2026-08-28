import json
import shutil
from pathlib import Path
from subprocess import STDOUT

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
    rendered = yaml.safe_load(
        (pkg / "info" / "recipe" / "rendered_recipe.yaml").read_text()
    )["recipe"]
    assert rendered["requirements"]["build"] == ["python"]
    assert rendered["requirements"]["host"] == ["zlib"]
    assert rendered["build"]["steps"][0]["name"] == "install"
    assert "overridden by recipe" in "\n".join(rendered["build"]["steps"][0]["run"])


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
        """steps:
  - name: compile
    requirements:
      build: [zlib]
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
          output.write("build.steps " + json.dumps([{"uses": "provider.yaml"}]) + "\\n")
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
    assert all(
        output["recipe"]["build"]["steps"][0]["resolved"]["steps"][0]["name"]
        == "compile"
        for output in rendered
    )


def test_metadata_rejects_new_variant_keys_in_multi_output_graphs(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Late matrix growth must not leave already rendered subpackage pins stale."""
    recipe_dir = tmp_path / "metadata-multi-output"
    recipe_dir.mkdir()
    (recipe_dir / "recipe.yaml").write_text(
        """schema_version: 1
recipe:
  name: metadata-multi-output
  version: 1.0.0
build:
  metadata:
    requirements:
      build: [python]
    interpreter: python
    run: |
      import os
      with open(os.environ["OUTPUT_FILE"], "w") as output:
          output.write('requirements.host.append ["zlib"]\\n')
outputs:
  - package:
      name: metadata-multi-output-child
"""
    )
    variant_config = tmp_path / "multi-output-variants.yaml"
    variant_config.write_text("zlib:\n  - 1.2\n  - 1.3\n")

    result = rattler_build(
        "build",
        "--recipe",
        str(recipe_dir),
        "--variant-config",
        str(variant_config),
        "--output-dir",
        str(tmp_path / "output"),
        "--render-only",
        "--experimental",
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "introduces variant key `zlib` in a multi-output recipe" in result.stderr
    assert "cannot safely recompute subpackage pins yet" in result.stderr


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

    assert "Generated metadata after build.metadata:" in output
    assert "# Final variant: metadata-step-example-1.0.0-" in output
    assert "Effective build steps for metadata-step-example" in output
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
    build_output = rattler_build(*build_args, stderr=STDOUT)
    assert "Generated metadata after build.metadata:" in build_output
    assert "- uses: python:build@==0.1.0" in build_output
    assert "summary: Render rich text" in build_output
    assert "Effective build steps for rich-14.2.0" in build_output
    assert "Found 1 final variants after build.metadata" in build_output
    assert "Running build step: python-build/build-wheel" in build_output
    assert "Ignoring prefix-detection" not in build_output
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
    assert rendered["requirements"]["host"] == [
        "python",
        "pip",
        "python-build",
        "poetry-core >=1.0.0",
    ]
    assert rendered["build"]["steps"][0]["name"] == "python-build"
    assert rendered["build"]["steps"][0]["uses"] == "python:build@==0.1.0"
    metadata_env = rendered["build"]["metadata"]["env"]
    assert "RATTLER_BUILD_PROVIDER_PREFIX" not in metadata_env
    assert metadata_env["RATTLER_BUILD_PROVIDER_VERSION"] == "0.1.0"
    assert (
        metadata_env["RATTLER_BUILD_BACKEND_ARGS"] == "--config-setting rich-build=true"
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
