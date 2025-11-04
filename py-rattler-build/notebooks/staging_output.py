import marimo

__generated_with = "0.17.6"
app = marimo.App()


@app.cell
def _():
    from rattler_build.stage0 import Recipe
    from rattler_build.variant_config import VariantConfig
    from rattler_build.render import render_recipe

    return Recipe, VariantConfig, render_recipe


@app.cell
def _(VariantConfig):
    _variant_config = VariantConfig()
    # should we _always_ have target_platform here?
    print(_variant_config)
    return


@app.cell
def _(Recipe, VariantConfig, render_recipe):
    _variant_config = VariantConfig()
    _variant_config.set_values("python", ["3.9", "3.10", "3.11"])
    _custom_recipe_yaml = '\nrecipe:\n  name: my-project\n  version: "1.0.0"\n\n# Top-level build: generate documentation\nbuild:\n  number: 0\n  script:\n    - echo "Generating docs..."\n    - echo "docs.html" > docs.html\n\noutputs:\n  # Staging: compile C++ library\n  - staging:\n      name: cpp-build\n    requirements:\n      build:\n        - ${{ compiler(\'cxx\') }}\n    build:\n      script:\n        - echo "Compiling C++ library..."\n        - echo "libmyproject.so" > libmyproject.so\n  \n  # Package 1: Library (from staging)\n  - package:\n      name: my-project-lib\n    inherit: cpp-build\n    build:\n      files:\n        - "*.so"\n        - "*.dll"\n    about:\n      summary: Compiled library\n  \n  # Package 2: Docs (from top-level)\n  - package:\n      name: my-project-docs\n    inherit: null\n    build:\n      noarch: generic\n      files:\n        - "*.html"\n        - "docs/**"\n    about:\n      summary: Documentation files\n  \n  # Package 3: Full package (from staging + extras)\n  - package:\n      name: my-project-py\n    inherit: cpp-build\n    requirements:\n      host:\n        - my-project-lib\n        - python\n    about:\n      summary: Full project with library and tools python bindings\n'
    _custom_recipe = Recipe.from_yaml(_custom_recipe_yaml)
    _variant_config = VariantConfig()
    _variant_config.set_values("python", ["3.9", "3.10", "3.11"])
    _custom_rendered = render_recipe(_custom_recipe, _variant_config)
    print(_custom_rendered)
    for _v in _custom_rendered:
        print(_v.variant())
    return


@app.cell
def _(Recipe, VariantConfig, render_recipe):
    _variant_config = VariantConfig()
    _variant_config.set_values("python", ["3.9", "3.10", "3.11"])
    _custom_recipe_yaml = '\nrecipe:\n  name: my-project\n  version: "1.0.0"\n\n# Top-level build: generate documentation\nbuild:\n  number: 0\n  script:\n    - echo "Generating docs..."\n    - echo "docs.html" > docs.html\n\nsource:\n    url: https://example.com/my-project-1.0.0.tar.gz\n    sha256: abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890\n\noutputs:\n  # Staging: compile C++ library\n  - staging:\n      name: cpp-build\n    requirements:\n      build:\n        - ${{ compiler(\'cxx\') }}\n      host:\n        - python\n    build:\n      script:\n        - echo "Compiling C++ library..."\n        - echo "libmyproject.so" > libmyproject.so\n  \n  # Package 1: Library (from staging)\n  - package:\n      name: my-project-lib\n    inherit: cpp-build\n    build:\n      files:\n        - "*.so"\n        - "*.dll"\n    about:\n      summary: Compiled library\n'
    _custom_recipe = Recipe.from_yaml(_custom_recipe_yaml)
    _custom_rendered = render_recipe(_custom_recipe, _variant_config)
    print(_custom_rendered)
    print(_custom_rendered[0].recipe().sources)
    print(_custom_rendered[0].recipe().staging_caches[0])
    cache = _custom_rendered[0].recipe().staging_caches[0]
    print(cache.sources)
    for s in cache.sources:
        print(s)
        print(s.to_dict())
    print(cache.build.script)
    for _v in _custom_rendered:
        # cannot access sources properly as _Source_ type
        # should have 3 variants for python 3.9, 3.10, 3.11 (from staging output)
        # Note: each variant should also contain target_platform
        print(_v.variant())
    return


if __name__ == "__main__":
    app.run()
