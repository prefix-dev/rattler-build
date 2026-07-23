from rattler_build import Stage0Recipe
from rattler_build.jinja_config import JinjaConfig
from rattler_build.render import render_context

RECIPE = """
context:
  name: mypkg
  version: "1.2.3"
  major: '${{ (version | split("."))[0] }}'
package:
  name: ${{ name }}
  version: ${{ version }}
build:
  number: 0
  string: ${{ major }}_${{ unknown_var }}
requirements:
  build:
    - ${{ compiler('c') }}
  run:
    - ${{ pin_subpackage('mypkg') }}
"""


def test_render_context_resolves_context_and_preserves_the_rest() -> None:
    rendered = render_context(Stage0Recipe.from_yaml(RECIPE))

    # `context` entries are evaluated; substituted scalars stay strings.
    assert rendered["context"]["major"] == "1"
    # Plain variables are substituted.
    assert rendered["package"]["name"] == "mypkg"
    assert rendered["package"]["version"] == "1.2.3"
    # A mixed scalar substitutes the known part and keeps the unknown verbatim.
    assert rendered["build"]["string"] == "1_${{ unknown_var }}"
    # Build-phase helper functions are left verbatim for the caller to handle.
    assert rendered["requirements"]["build"] == ["${{ compiler('c') }}"]
    assert rendered["requirements"]["run"] == ["${{ pin_subpackage('mypkg') }}"]


def test_render_context_accepts_a_plain_dict_and_preserves_structure() -> None:
    recipe = {
        "context": {"name": "abc"},
        "package": {"name": "${{ name }}", "version": "${{ missing }}"},
    }
    rendered = render_context(recipe)
    assert rendered == {
        "context": {"name": "abc"},
        "package": {"name": "abc", "version": "${{ missing }}"},
    }


def test_render_context_uses_jinja_config_platform() -> None:
    recipe = {"context": {"sel": "${{ 'yes' if linux else 'no' }}"}}
    from rattler_build.tool_config import PlatformConfig

    rendered = render_context(recipe, JinjaConfig(platform=PlatformConfig(target_platform="linux-64")))
    assert rendered["context"]["sel"] == "yes"


def test_render_context_feeds_rendered_strings_forward() -> None:
    recipe = {
        "context": {
            "version": "0.2025.39",
            "major": '${{ (version | split("."))[0] }}',
            "tag": "${{ 'weekly' if major == '0' else 'release' }}",
            "combo": "${{ unknown_thing }}",
            "ref": "prefix-${{ combo }}",
        }
    }

    rendered = render_context(recipe)

    # Later entries see the rendered *string*, like a Jinja engine would.
    assert rendered["context"]["major"] == "0"
    assert rendered["context"]["tag"] == "weekly"
    # Unresolved entries stay verbatim and references to them are not expanded.
    assert rendered["context"]["combo"] == "${{ unknown_thing }}"
    assert rendered["context"]["ref"] == "prefix-${{ combo }}"


def test_render_context_with_functions_evaluates_mapped_helpers() -> None:
    rendered = render_context(
        Stage0Recipe.from_yaml(RECIPE),
        functions={
            "compiler": lambda lang: f"{lang}_compiler_stub",
            "pin_subpackage": lambda name, **kwargs: f"subpackage_pin {name}",
        },
    )

    # Mapped helpers are evaluated with the callable's return value.
    assert rendered["requirements"]["build"] == ["c_compiler_stub"]
    assert rendered["requirements"]["run"] == ["subpackage_pin mypkg"]
    # Everything else keeps the default lenient behavior.
    assert rendered["package"]["name"] == "mypkg"
    assert rendered["build"]["string"] == "1_${{ unknown_var }}"


def test_render_context_with_functions_forwards_kwargs() -> None:
    recipe = {
        "context": {"name": "abc"},
        "requirements": {"run": ['${{ pin_subpackage("abc", exact=True) }}']},
    }

    def pin_subpackage(name: str, exact: bool = False) -> str:
        return f"subpackage_pin {name} exact={exact}"

    rendered = render_context(recipe, functions={"pin_subpackage": pin_subpackage})
    assert rendered["requirements"]["run"] == ["subpackage_pin abc exact=True"]


def test_render_context_with_raising_function_preserves_expression() -> None:
    def broken(*args: object, **kwargs: object) -> str:
        raise ValueError("boom")

    recipe = {"requirements": {"build": ["${{ compiler('c') }}"]}}
    rendered = render_context(recipe, functions={"compiler": broken})
    assert rendered["requirements"]["build"] == ["${{ compiler('c') }}"]


def test_render_context_keeps_substituted_scalars_as_strings() -> None:
    recipe = {
        "context": {"build_num": 5, "python_min": "3.10"},
        "build": {"number": "${{ build_num }}"},
        "tests": [{"python": {"python_version": "${{ python_min }}"}}],
    }

    rendered = render_context(recipe)
    # Recovering scalar types is the caller's job: reading these back as YAML
    # needs the source quoting to know "3.10" is a version and not the float 3.1.
    assert rendered["build"]["number"] == "5"
    assert rendered["tests"][0]["python"]["python_version"] == "3.10"
    # An untemplated scalar keeps whatever type it had.
    assert rendered["context"]["build_num"] == 5
