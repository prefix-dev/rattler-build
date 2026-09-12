import shutil
import tarfile
from pathlib import Path
from subprocess import STDOUT, CalledProcessError

import pytest
import yaml
from helpers import RattlerBuild, get_extracted_package, get_package


def action_project(tmp_path: Path, action, *, arguments=None, context=None, steps=None):
    project = tmp_path / "project"
    project.mkdir()
    recipe = {
        "schema_version": 1,
        "package": {"name": "action-contract", "version": "1.0"},
        "build": {
            "steps": steps
            if steps is not None
            else [{"name": "test", "uses": "./action.yaml", "with": arguments or {}}]
        },
    }
    if context is not None:
        recipe["context"] = context
    (project / "recipe.yaml").write_text(yaml.safe_dump(recipe))
    (project / "action.yaml").write_text(yaml.safe_dump(action))
    return project


def run_action(rattler_build: RattlerBuild, project: Path, output: Path, name="test"):
    return rattler_build(
        "run",
        name,
        "--recipe",
        str(project),
        "--source-dir",
        str(project),
        "--output-dir",
        str(output),
        "--experimental",
    )


@pytest.mark.parametrize("provider", ["cmake", "meson", "rust", "go"])
@pytest.mark.parametrize("enabled", [False, True])
def test_example_providers_use_typed_actions_and_step_conditions(
    rattler_build: RattlerBuild,
    recipes: Path,
    tmp_path: Path,
    provider: str,
    enabled: bool,
):
    source = recipes.parents[1] / "examples" / "step-providers" / "providers" / provider
    flag = "collect_licenses" if provider == "go" else "install"
    project = action_project(
        tmp_path,
        yaml.safe_load((source / "build.yaml").read_text()),
        arguments={flag: enabled},
    )
    rendered = rattler_build.render(
        project, tmp_path / "output", extra_args=["--experimental"]
    )
    steps = rendered[0]["recipe"]["build"]["steps"]
    optional_step = (
        "go-licenses"
        if provider == "go"
        else f"{'cargo' if provider == 'rust' else provider}-install"
    )
    assert any(step["name"].endswith("/" + optional_step) for step in steps) == enabled


def test_nested_action_selection_and_relative_resolution(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {"steps": []},
        steps=[
            {"name": "prepare", "run": "echo prepare > order.txt"},
            {"name": "test", "uses": "./nested/outer.yaml", "depends_on": ["prepare"]},
            {"name": "unselected", "run": "echo wrong >> order.txt"},
        ],
    )
    nested = project / "nested"
    nested.mkdir()
    (nested / "outer.yaml").write_text(
        yaml.safe_dump(
            {
                "steps": [
                    {"uses": "./child.yml", "with": {"message": "first"}},
                    {"uses": "./child.yml", "with": {"message": "second"}},
                    {"run": "echo outer >> order.txt"},
                ]
            }
        )
    )
    (nested / "child.yml").write_text(
        yaml.safe_dump(
            {
                "inputs": {"message": {"type": "string"}},
                "steps": [{"run": "echo ${{ inputs.message }} >> order.txt"}],
            }
        )
    )
    run_action(rattler_build, project, tmp_path / "output")
    assert [
        line.strip() for line in (project / "order.txt").read_text().splitlines()
    ] == [
        "prepare",
        "first",
        "second",
        "outer",
    ]


def test_empty_action_keeps_named_dependencies_and_requirements(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "requirements": {"host": ["python"]},
            "steps": [],
        },
        steps=[
            {"name": "prepare", "run": "echo prepared > result.txt"},
            {"name": "test", "uses": "./action.yaml", "depends_on": ["prepare"]},
        ],
    )
    (project / "variants.yaml").write_text('python: ["3.11", "3.12"]\n')
    rendered = rattler_build.render(
        project, tmp_path / "render", extra_args=["--experimental"]
    )
    assert {item["build_configuration"]["variant"]["python"] for item in rendered} == {
        "3.11",
        "3.12",
    }
    (project / "variants.yaml").write_text('python: ["3.11"]\n')
    run_action(rattler_build, project, tmp_path / "output")
    assert (project / "result.txt").read_text().strip() == "prepared"


@pytest.mark.parametrize("reference", ["./missing.yaml", "missing-provider:build@0"])
def test_skipped_output_does_not_resolve_actions(
    rattler_build: RattlerBuild, tmp_path: Path, reference: str
):
    project = action_project(tmp_path, {"steps": []}, steps=[{"uses": reference}])
    path = project / "recipe.yaml"
    recipe = yaml.safe_load(path.read_text())
    recipe["build"]["skip"] = True
    path.write_text(yaml.safe_dump(recipe))
    rendered = rattler_build.render(
        project, tmp_path / "output", extra_args=["--experimental"]
    )
    assert rendered == []
    recipe["build"]["skip"] = False
    path.write_text(yaml.safe_dump(recipe))
    with pytest.raises(CalledProcessError):
        rattler_build.render(
            project, tmp_path / "output", extra_args=["--experimental"]
        )


def test_false_invocation_skips_resolution_and_input_validation(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "steps": [
                {
                    "uses": "./missing.yaml",
                    "if": "false",
                    "with": {"bad": "${{ missing_value }}"},
                },
                {"run": "echo included > result.txt"},
            ]
        },
    )
    run_action(rattler_build, project, tmp_path / "output")
    assert (project / "result.txt").read_text().strip() == "included"


@pytest.mark.parametrize("cycle", [True, False], ids=["cycle", "depth-limit"])
def test_recursive_action_errors_before_execution(
    rattler_build: RattlerBuild, tmp_path: Path, cycle
):
    project = action_project(
        tmp_path,
        {
            "steps": [
                {"run": "echo must-not-run > result.txt"},
                {"uses": "./next-0.yaml"},
            ]
        },
    )
    count = 2 if cycle else 65
    for index in range(count):
        following = (
            "./action.yaml"
            if cycle and index == count - 1
            else f"./next-{index + 1}.yaml"
        )
        steps = [{"uses": following}] if cycle or index < count - 1 else []
        (project / f"next-{index}.yaml").write_text(yaml.safe_dump({"steps": steps}))
    with pytest.raises(CalledProcessError):
        run_action(rattler_build, project, tmp_path / "output")
    assert not (project / "result.txt").exists()


def test_action_inputs_preserve_native_types_and_punctuation(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "inputs": {
                "text": {"type": "string"},
                "numbers": {"type": "list", "items": "integer"},
                "enabled": {"type": "boolean"},
                "optional": {"type": "string", "default": "fallback"},
            },
            "steps": [
                {
                    "run": "echo ${{ inputs.text }}-${{ inputs.numbers[0] + inputs.numbers[1] }}-${{ 1 if inputs.enabled else 0 }}-${{ 'null' if inputs.optional is none else inputs.optional }} > result.txt"
                }
            ],
        },
        arguments={
            "text": "status: ready",
            "numbers": ["${{ count }}", 2],
            "enabled": "${{ enabled }}",
            "optional": None,
        },
        context={"count": 7, "enabled": False},
    )
    run_action(rattler_build, project, tmp_path / "output")
    assert (project / "result.txt").read_text().strip() == "status: ready-9-0-null"


def test_action_variants_and_inputs_have_separate_namespaces(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "inputs": {"python": {"type": "string"}},
            "steps": [
                {
                    "run": "echo ${{ python }}-${{ inputs.python }}-${{ 'leaked' if private_value is defined else 'isolated' }} > result.txt"
                }
            ],
        },
        arguments={"python": "${{ private_value }}"},
        context={"private_value": "argument", "python": "recipe-shadow"},
    )
    (project / "variants.yaml").write_text('python: ["3.11"]\n')
    run_action(rattler_build, project, tmp_path / "output")
    assert (project / "result.txt").read_text().strip() == "3.11-argument-isolated"


def test_action_bare_requirements_expand_normal_recipe_variants(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "requirements": {"host": ["python"]},
            "steps": [
                {"run": "echo ${{ python }}"},
            ],
        },
    )
    (project / "variants.yaml").write_text('python: ["3.11", "3.12"]\n')
    rendered = rattler_build.render(
        project, tmp_path / "output", extra_args=["--experimental"]
    )
    assert {item["build_configuration"]["variant"]["python"] for item in rendered} == {
        "3.11",
        "3.12",
    }
    assert len({item["recipe"]["build"]["string"] for item in rendered}) == 2


def test_nested_bindings_remain_isolated_with_late_bound_values(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "inputs": {"message": {"type": "string"}},
            "steps": [
                {"uses": "./child.yaml", "with": {"message": "child"}},
                {"run": "echo ${{ inputs.message }} > parent.txt"},
            ],
        },
        arguments={"message": "parent"},
    )
    (project / "child.yaml").write_text(
        yaml.safe_dump(
            {
                "inputs": {"message": {"type": "string"}},
                "steps": [
                    {"run": "echo ${{ inputs.message }} > child.txt"},
                    {"run": "echo ${{ BUILD_DIR }} > spaced.txt"},
                    {"run": "echo ${{BUILD_DIR}} > compact.txt"},
                ],
            }
        )
    )
    run_action(rattler_build, project, tmp_path / "output")
    assert (project / "parent.txt").read_text().strip() == "parent"
    assert (project / "child.txt").read_text().strip() == "child"
    spaced = (project / "spaced.txt").read_text().strip()
    assert Path(spaced).is_dir()
    assert (project / "compact.txt").read_text().strip() == spaced


@pytest.mark.parametrize(
    "declaration,value",
    [
        ({}, "value"),
        ({"type": "string", "required": True, "default": "value"}, "value"),
        ({"type": "integer"}, "7"),
        ({"type": "integer"}, True),
        ({"type": "list", "items": "integer"}, [1, "2"]),
        ({"type": "list", "items": "integer"}, [None]),
        ({"type": "list"}, []),
        ({"type": "string", "items": "string"}, "value"),
        ({"type": "string", "default": "${{ python }}"}, "value"),
        ({"type": "string"}, None),
    ],
)
def test_action_rejects_invalid_typed_arguments(
    rattler_build: RattlerBuild, tmp_path: Path, declaration, value
):
    project = action_project(
        tmp_path,
        {
            "inputs": {"value": declaration},
            "steps": [{"run": "echo invalid > result.txt"}],
        },
        arguments={"value": value},
    )
    with pytest.raises(CalledProcessError):
        run_action(rattler_build, project, tmp_path / "output")
    assert not (project / "result.txt").exists()


@pytest.mark.parametrize(
    "action",
    [
        {"schema_version": 999, "steps": []},
        {"unknown": True, "steps": []},
        {"action": {"unknown": True}, "steps": []},
        {"requirements": {"run": ["python"]}, "steps": []},
        {"inputs": {"value": {"type": "string"}}, "steps": []},
        {"inputs": {"bad-name": {"type": "string"}}, "steps": []},
        {"schema_version": 1},
    ],
)
def test_action_rejects_invalid_documents(
    rattler_build: RattlerBuild, tmp_path: Path, action
):
    project = action_project(tmp_path, action)
    with pytest.raises(CalledProcessError):
        rattler_build.render(
            project, tmp_path / "output", extra_args=["--experimental"]
        )


@pytest.mark.parametrize(
    "override",
    [
        {"env": {"A": "B"}},
        {"cwd": "child"},
        {"interpreter": "python"},
        {"requirements": {"build": ["python"]}},
    ],
)
def test_action_invocation_rejects_execution_overrides(
    rattler_build: RattlerBuild, tmp_path: Path, override
):
    project = action_project(
        tmp_path,
        {"steps": []},
        steps=[
            {"uses": "./action.yaml", **override},
        ],
    )
    with pytest.raises(CalledProcessError):
        rattler_build.render(
            project, tmp_path / "output", extra_args=["--experimental"]
        )


def test_compiled_action_rebuild_does_not_need_action_documents(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "inputs": {"message": {"type": "string"}},
            "steps": [
                {"run": 'echo ${{ inputs.message }}> "${{ PREFIX }}/marker.txt"'}
            ],
        },
        arguments={"message": "compiled-action"},
    )
    original_output = tmp_path / "original"
    rattler_build.build(project, original_output, extra_args=["--experimental"])
    original_package = get_package(original_output, "action-contract")
    assert (
        get_extracted_package(original_output, "action-contract") / "marker.txt"
    ).read_text().strip() == "compiled-action"
    stripped = tmp_path / original_package.name
    with (
        tarfile.open(original_package, "r:bz2") as source,
        tarfile.open(stripped, "w:bz2") as target,
    ):
        for member in source:
            if (
                member.name.startswith("info/recipe/")
                and member.name != "info/recipe/rendered_recipe.yaml"
            ):
                continue
            target.addfile(
                member, source.extractfile(member) if member.isfile() else None
            )
    shutil.rmtree(project)
    rebuilt_output = tmp_path / "rebuilt"
    rattler_build(
        "rebuild",
        "--package-file",
        str(stripped),
        "--output-dir",
        str(rebuilt_output),
        "--experimental",
        "--test=skip",
        stderr=STDOUT,
    )
    assert (
        get_extracted_package(rebuilt_output, "action-contract") / "marker.txt"
    ).read_text().strip() == "compiled-action"


def test_staging_actions_share_package_compilation(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "steps": [{"run": 'echo staged> "${{ PREFIX }}/staged.txt"'}],
        },
    )
    (project / "recipe.yaml").write_text(
        yaml.safe_dump(
            {
                "recipe": {"name": "action-staging", "version": "1.0"},
                "build": {"steps": [{"uses": "./unused.yaml"}]},
                "outputs": [
                    {
                        "staging": {"name": "prepared"},
                        "build": {"steps": [{"uses": "./action.yaml"}]},
                    },
                    {
                        "package": {"name": "action-staging"},
                        "inherit": "prepared",
                    },
                ],
            }
        )
    )
    output = tmp_path / "output"
    rattler_build.build(project, output, extra_args=["--experimental"])
    assert (
        get_extracted_package(output, "action-staging") / "staged.txt"
    ).read_text().strip() == "staged"


def test_action_cannot_defer_private_recipe_context_to_execution(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {
            "steps": [{"run": "echo ${{ private_value }} > result.txt"}],
        },
        context={"private_value": "secret"},
    )
    with pytest.raises(CalledProcessError):
        run_action(rattler_build, project, tmp_path / "output")
    assert not (project / "result.txt").exists()


def test_excluded_action_does_not_consume_argument_variants(
    rattler_build: RattlerBuild, tmp_path: Path
):
    project = action_project(
        tmp_path,
        {"steps": [{"run": "echo selected"}]},
        steps=[
            {
                "if": "false",
                "uses": "./missing.yaml",
                "with": {"version": "${{ python }}"},
            },
            {"uses": "./action.yaml"},
        ],
    )
    (project / "variants.yaml").write_text('python: ["3.11", "3.12"]\n')
    rendered = rattler_build.render(
        project, tmp_path / "output", extra_args=["--experimental"]
    )
    assert [
        item["build_configuration"]["variant"].get("python") for item in rendered
    ] == [None]


def test_named_execution_rejects_script_mode(
    rattler_build: RattlerBuild, tmp_path: Path
):
    (tmp_path / "recipe.yaml").write_text(
        yaml.safe_dump(
            {
                "package": {"name": "script-selection", "version": "1"},
                "build": {"script": "echo unexpected > marker.txt"},
            }
        )
    )
    with pytest.raises(CalledProcessError):
        run_action(rattler_build, tmp_path, tmp_path / "output", name="missing")
    assert not (tmp_path / "marker.txt").exists()


def test_output_selection_ignores_discarded_top_level_actions(
    rattler_build: RattlerBuild, tmp_path: Path
):
    (tmp_path / "recipe.yaml").write_text(
        yaml.safe_dump(
            {
                "schema_version": 1,
                "recipe": {"name": "output-selection", "version": "1"},
                "build": {"steps": [{"name": "unused", "uses": "./missing.yaml"}]},
                "outputs": [
                    {
                        "package": {"name": "output-selection"},
                        "build": {
                            "steps": [
                                {
                                    "name": "check",
                                    "run": "echo selected > marker.txt",
                                }
                            ]
                        },
                    }
                ],
            }
        )
    )
    run_action(rattler_build, tmp_path, tmp_path / "output", name="check")
    assert (tmp_path / "marker.txt").read_text().strip() == "selected"
