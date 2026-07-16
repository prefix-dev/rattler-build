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

    # `context` entries are evaluated (and typed like YAML would read them back).
    assert rendered["context"]["major"] == 1
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

    rendered = render_context(
        recipe, JinjaConfig(platform=PlatformConfig(target_platform="linux-64"))
    )
    assert rendered["context"]["sel"] == "yes"
