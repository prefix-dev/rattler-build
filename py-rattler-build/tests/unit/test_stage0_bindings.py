"""Tests for Stage0 Python bindings."""

import pytest
from inline_snapshot import snapshot
from rattler_build.stage0 import Recipe, SingleOutputRecipe, MultiOutputRecipe


# Sample YAML recipes for testing
SIMPLE_RECIPE_YAML = """
package:
  name: test-package
  version: 1.0.0

build:
  number: 0

requirements:
  host:
    - python
  run:
    - python

about:
  summary: A test package
  license: MIT
"""

MULTI_OUTPUT_RECIPE_YAML = """
recipe:
  name: test-multi
  version: 1.0.0

build:
  number: 0

outputs:
  - package:
      name: test-multi-lib
    requirements:
      run:
        - libtest
    
  - package:
      name: test-multi-dev
    requirements:
      run:
        - test-multi-lib

about:
  summary: A multi-output test package
  license: MIT
"""


def test_recipe_from_yaml_single_output() -> None:
    """Test parsing a single-output recipe from YAML."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)

    assert recipe is not None
    assert isinstance(recipe, SingleOutputRecipe)
    assert not isinstance(recipe, MultiOutputRecipe)


def test_recipe_from_yaml_multi_output() -> None:
    """Test parsing a multi-output recipe from YAML."""
    recipe = Recipe.from_yaml(MULTI_OUTPUT_RECIPE_YAML)

    assert recipe is not None
    assert isinstance(recipe, MultiOutputRecipe)
    assert not isinstance(recipe, SingleOutputRecipe)


def test_recipe_to_dict() -> None:
    """Test converting recipe to dictionary."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)

    recipe_dict = recipe.to_dict()
    assert isinstance(recipe_dict, dict)
    assert "package" in recipe_dict


def test_recipe_to_dict_snapshot() -> None:
    """Test recipe serialization with snapshot."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)

    recipe_dict = recipe.to_dict()
    # The snapshot will capture the exact structure
    assert recipe_dict == snapshot(
        {
            "package": {"name": "test-package", "version": "1.0.0"},
            "build": {
                "number": 0,
                "string": None,
                "script": {},
                "noarch": None,
                "python": {
                    "entry_points": [],
                    "skip_pyc_compilation": [],
                    "use_python_app_entrypoint": False,
                    "version_independent": False,
                    "site_packages_path": None,
                },
                "skip": [],
                "always_copy_files": [],
                "always_include_files": [],
                "merge_build_and_host_envs": False,
                "files": [],
                "dynamic_linking": {
                    "rpaths": [],
                    "binary_relocation": True,
                    "missing_dso_allowlist": [],
                    "rpath_allowlist": [],
                    "overdepending_behavior": None,
                    "overlinking_behavior": None,
                },
                "variant": {
                    "use_keys": [],
                    "ignore_keys": [],
                    "down_prioritize_variant": None,
                },
                "prefix_detection": {
                    "force_file_type": {"text": [], "binary": []},
                    "ignore": False,
                    "ignore_binary_files": False,
                },
                "post_process": [],
            },
            "requirements": {"host": ["python"], "run": ["python"]},
            "about": {
                "homepage": None,
                "license": "MIT",
                "license_family": None,
                "summary": "A test package",
                "description": None,
                "documentation": None,
                "repository": None,
            },
            "extra": {},
        }
    )


def test_single_output_recipe_package() -> None:
    """Test accessing package info from single-output recipe."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)

    assert isinstance(recipe, SingleOutputRecipe)
    assert recipe.package is not None
    assert recipe.build is not None
    assert recipe.requirements is not None
    assert recipe.about is not None

    # TODO: decide if we want to keep context a dictionary
    context = recipe.context
    assert isinstance(context, dict)


def test_single_output_recipe_to_dict() -> None:
    """Test converting single-output recipe to dict."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    recipe_dict = recipe.to_dict()
    assert isinstance(recipe_dict, dict)
    assert "package" in recipe_dict


def test_single_output_recipe_to_dict_snapshot() -> None:
    """Test single-output recipe serialization with snapshot."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    recipe_dict = recipe.to_dict()

    # Snapshot the full structure
    assert recipe_dict == snapshot(
        {
            "package": {"name": "test-package", "version": "1.0.0"},
            "build": {
                "number": 0,
                "string": None,
                "script": {},
                "noarch": None,
                "python": {
                    "entry_points": [],
                    "skip_pyc_compilation": [],
                    "use_python_app_entrypoint": False,
                    "version_independent": False,
                    "site_packages_path": None,
                },
                "skip": [],
                "always_copy_files": [],
                "always_include_files": [],
                "merge_build_and_host_envs": False,
                "files": [],
                "dynamic_linking": {
                    "rpaths": [],
                    "binary_relocation": True,
                    "missing_dso_allowlist": [],
                    "rpath_allowlist": [],
                    "overdepending_behavior": None,
                    "overlinking_behavior": None,
                },
                "variant": {
                    "use_keys": [],
                    "ignore_keys": [],
                    "down_prioritize_variant": None,
                },
                "prefix_detection": {
                    "force_file_type": {"text": [], "binary": []},
                    "ignore": False,
                    "ignore_binary_files": False,
                },
                "post_process": [],
            },
            "requirements": {"host": ["python"], "run": ["python"]},
            "about": {
                "homepage": None,
                "license": "MIT",
                "license_family": None,
                "summary": "A test package",
                "description": None,
                "documentation": None,
                "repository": None,
            },
            "extra": {},
        }
    )


def test_multi_output_recipe_about() -> None:
    """Test accessing about from multi-output recipe."""
    recipe = Recipe.from_yaml(MULTI_OUTPUT_RECIPE_YAML)

    assert isinstance(recipe, MultiOutputRecipe)
    assert isinstance(recipe.outputs, list)
    assert len(recipe.outputs) == 2

    assert recipe.recipe is not None
    assert recipe.about is not None
    assert recipe.build is not None

    context = recipe.context
    assert isinstance(context, dict)


def test_multi_output_recipe_to_dict() -> None:
    """Test converting multi-output recipe to dict."""
    recipe = Recipe.from_yaml(MULTI_OUTPUT_RECIPE_YAML)
    recipe_dict = recipe.to_dict()
    assert isinstance(recipe_dict, dict)
    assert "recipe" in recipe_dict or "outputs" in recipe_dict


def test_multi_output_recipe_to_dict_snapshot() -> None:
    """Test multi-output recipe serialization with snapshot."""
    recipe = Recipe.from_yaml(MULTI_OUTPUT_RECIPE_YAML)

    # Snapshot the full structure including all outputs
    assert recipe.to_dict() == snapshot(
        {
            "recipe": {"name": "test-multi", "version": "1.0.0"},
            "build": {
                "number": 0,
                "string": None,
                "script": {},
                "noarch": None,
                "python": {
                    "entry_points": [],
                    "skip_pyc_compilation": [],
                    "use_python_app_entrypoint": False,
                    "version_independent": False,
                    "site_packages_path": None,
                },
                "skip": [],
                "always_copy_files": [],
                "always_include_files": [],
                "merge_build_and_host_envs": False,
                "files": [],
                "dynamic_linking": {
                    "rpaths": [],
                    "binary_relocation": True,
                    "missing_dso_allowlist": [],
                    "rpath_allowlist": [],
                    "overdepending_behavior": None,
                    "overlinking_behavior": None,
                },
                "variant": {
                    "use_keys": [],
                    "ignore_keys": [],
                    "down_prioritize_variant": None,
                },
                "prefix_detection": {
                    "force_file_type": {"text": [], "binary": []},
                    "ignore": False,
                    "ignore_binary_files": False,
                },
                "post_process": [],
            },
            "about": {
                "homepage": None,
                "license": "MIT",
                "license_family": None,
                "summary": "A multi-output test package",
                "description": None,
                "documentation": None,
                "repository": None,
            },
            "extra": {},
            "outputs": [
                {
                    "package": {"name": "test-multi-lib"},
                    "inherit": None,
                    "requirements": {"run": ["libtest"]},
                    "build": {
                        "number": 0,
                        "string": None,
                        "script": {},
                        "noarch": None,
                        "python": {
                            "entry_points": [],
                            "skip_pyc_compilation": [],
                            "use_python_app_entrypoint": False,
                            "version_independent": False,
                            "site_packages_path": None,
                        },
                        "skip": [],
                        "always_copy_files": [],
                        "always_include_files": [],
                        "merge_build_and_host_envs": False,
                        "files": [],
                        "dynamic_linking": {
                            "rpaths": [],
                            "binary_relocation": True,
                            "missing_dso_allowlist": [],
                            "rpath_allowlist": [],
                            "overdepending_behavior": None,
                            "overlinking_behavior": None,
                        },
                        "variant": {
                            "use_keys": [],
                            "ignore_keys": [],
                            "down_prioritize_variant": None,
                        },
                        "prefix_detection": {
                            "force_file_type": {"text": [], "binary": []},
                            "ignore": False,
                            "ignore_binary_files": False,
                        },
                        "post_process": [],
                    },
                    "about": {
                        "homepage": None,
                        "license": None,
                        "license_family": None,
                        "summary": None,
                        "description": None,
                        "documentation": None,
                        "repository": None,
                    },
                },
                {
                    "package": {"name": "test-multi-dev"},
                    "inherit": None,
                    "requirements": {"run": ["test-multi-lib"]},
                    "build": {
                        "number": 0,
                        "string": None,
                        "script": {},
                        "noarch": None,
                        "python": {
                            "entry_points": [],
                            "skip_pyc_compilation": [],
                            "use_python_app_entrypoint": False,
                            "version_independent": False,
                            "site_packages_path": None,
                        },
                        "skip": [],
                        "always_copy_files": [],
                        "always_include_files": [],
                        "merge_build_and_host_envs": False,
                        "files": [],
                        "dynamic_linking": {
                            "rpaths": [],
                            "binary_relocation": True,
                            "missing_dso_allowlist": [],
                            "rpath_allowlist": [],
                            "overdepending_behavior": None,
                            "overlinking_behavior": None,
                        },
                        "variant": {
                            "use_keys": [],
                            "ignore_keys": [],
                            "down_prioritize_variant": None,
                        },
                        "prefix_detection": {
                            "force_file_type": {"text": [], "binary": []},
                            "ignore": False,
                            "ignore_binary_files": False,
                        },
                        "post_process": [],
                    },
                    "about": {
                        "homepage": None,
                        "license": None,
                        "license_family": None,
                        "summary": None,
                        "description": None,
                        "documentation": None,
                        "repository": None,
                    },
                },
            ],
        }
    )


def test_package_to_dict() -> None:
    """Test converting package to dict."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    assert isinstance(recipe, SingleOutputRecipe)
    package = recipe.package
    package_dict = package.to_dict()
    assert isinstance(package_dict, dict)


def test_package_to_dict_snapshot() -> None:
    """Test package serialization with snapshot."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    assert isinstance(recipe, SingleOutputRecipe)

    package = recipe.package
    package_dict = package.to_dict()

    # Snapshot should capture name and version
    assert package_dict == snapshot({"name": "test-package", "version": "1.0.0"})


def test_build_to_dict_snapshot() -> None:
    """Test build serialization with snapshot."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    assert isinstance(recipe, SingleOutputRecipe)

    build = recipe.build
    build_dict = build.to_dict()

    # Snapshot should capture build number and other build settings
    assert build_dict == snapshot(
        {
            "number": 0,
            "string": None,
            "script": {},
            "noarch": None,
            "python": {
                "entry_points": [],
                "skip_pyc_compilation": [],
                "use_python_app_entrypoint": False,
                "version_independent": False,
                "site_packages_path": None,
            },
            "skip": [],
            "always_copy_files": [],
            "always_include_files": [],
            "merge_build_and_host_envs": False,
            "files": [],
            "dynamic_linking": {
                "rpaths": [],
                "binary_relocation": True,
                "missing_dso_allowlist": [],
                "rpath_allowlist": [],
                "overdepending_behavior": None,
                "overlinking_behavior": None,
            },
            "variant": {
                "use_keys": [],
                "ignore_keys": [],
                "down_prioritize_variant": None,
            },
            "prefix_detection": {
                "force_file_type": {"text": [], "binary": []},
                "ignore": False,
                "ignore_binary_files": False,
            },
            "post_process": [],
        }
    )


def test_requirements_to_dict_snapshot() -> None:
    """Test requirements serialization with snapshot."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    assert isinstance(recipe, SingleOutputRecipe)

    requirements = recipe.requirements
    requirements_dict = requirements.to_dict()

    # Snapshot should capture host and run dependencies
    assert requirements_dict == snapshot({"host": ["python"], "run": ["python"]})


def test_about_to_dict_snapshot() -> None:
    """Test about serialization with snapshot."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)
    assert isinstance(recipe, SingleOutputRecipe)

    about = recipe.about
    about_dict = about.to_dict()

    # Snapshot should capture summary and license
    assert about_dict == snapshot(
        {
            "homepage": None,
            "license": "MIT",
            "license_family": None,
            "summary": "A test package",
            "description": None,
            "documentation": None,
            "repository": None,
        }
    )


def test_recipe_with_context() -> None:
    """Test recipe with Jinja context variables."""
    yaml_with_context = """
context:
  version: 1.2.3

package:
  name: test-package
  version: ${{ version }}

build:
  number: 0
"""

    recipe = Recipe.from_yaml(yaml_with_context)
    context = recipe.context
    assert len(context) > 0


def test_invalid_yaml() -> None:
    """Test that invalid YAML raises an error."""
    invalid_yaml = """
    this is not: [valid yaml
    """

    with pytest.raises(Exception):  # Should raise some parsing error
        Recipe.from_yaml(invalid_yaml)


def test_recipe_repr() -> None:
    """Test recipe string representation."""
    recipe = Recipe.from_yaml(SIMPLE_RECIPE_YAML)

    repr_str = repr(recipe)
    assert "Stage0Recipe" in repr_str or "Recipe" in repr_str


def test_multi_output_outputs_snapshot() -> None:
    """Test multi-output recipe outputs structure with snapshot."""
    recipe = Recipe.from_yaml(MULTI_OUTPUT_RECIPE_YAML)
    assert isinstance(recipe, MultiOutputRecipe)
    outputs = recipe.outputs

    # Convert outputs to serializable format for snapshot
    outputs_data = []
    for output in outputs:
        outputs_data.append(output.to_dict())

    # Snapshot should capture all output configurations
    assert outputs_data == snapshot(
        [
            {
                "package": {"name": "test-multi-lib"},
                "inherit": None,
                "requirements": {"run": ["libtest"]},
                "build": {
                    "number": 0,
                    "string": None,
                    "script": {},
                    "noarch": None,
                    "python": {
                        "entry_points": [],
                        "skip_pyc_compilation": [],
                        "use_python_app_entrypoint": False,
                        "version_independent": False,
                        "site_packages_path": None,
                    },
                    "skip": [],
                    "always_copy_files": [],
                    "always_include_files": [],
                    "merge_build_and_host_envs": False,
                    "files": [],
                    "dynamic_linking": {
                        "rpaths": [],
                        "binary_relocation": True,
                        "missing_dso_allowlist": [],
                        "rpath_allowlist": [],
                        "overdepending_behavior": None,
                        "overlinking_behavior": None,
                    },
                    "variant": {
                        "use_keys": [],
                        "ignore_keys": [],
                        "down_prioritize_variant": None,
                    },
                    "prefix_detection": {
                        "force_file_type": {"text": [], "binary": []},
                        "ignore": False,
                        "ignore_binary_files": False,
                    },
                    "post_process": [],
                },
                "about": {
                    "homepage": None,
                    "license": None,
                    "license_family": None,
                    "summary": None,
                    "description": None,
                    "documentation": None,
                    "repository": None,
                },
            },
            {
                "package": {"name": "test-multi-dev"},
                "inherit": None,
                "requirements": {"run": ["test-multi-lib"]},
                "build": {
                    "number": 0,
                    "string": None,
                    "script": {},
                    "noarch": None,
                    "python": {
                        "entry_points": [],
                        "skip_pyc_compilation": [],
                        "use_python_app_entrypoint": False,
                        "version_independent": False,
                        "site_packages_path": None,
                    },
                    "skip": [],
                    "always_copy_files": [],
                    "always_include_files": [],
                    "merge_build_and_host_envs": False,
                    "files": [],
                    "dynamic_linking": {
                        "rpaths": [],
                        "binary_relocation": True,
                        "missing_dso_allowlist": [],
                        "rpath_allowlist": [],
                        "overdepending_behavior": None,
                        "overlinking_behavior": None,
                    },
                    "variant": {
                        "use_keys": [],
                        "ignore_keys": [],
                        "down_prioritize_variant": None,
                    },
                    "prefix_detection": {
                        "force_file_type": {"text": [], "binary": []},
                        "ignore": False,
                        "ignore_binary_files": False,
                    },
                    "post_process": [],
                },
                "about": {
                    "homepage": None,
                    "license": None,
                    "license_family": None,
                    "summary": None,
                    "description": None,
                    "documentation": None,
                    "repository": None,
                },
            },
        ]
    )


def test_context_with_jinja_snapshot() -> None:
    """Test recipe context with Jinja variables using snapshot."""
    yaml_with_context = """
context:
  version: 1.2.3
  name: my-package

package:
  name: ${{ name }}
  version: ${{ version }}

requirements:
  build:
    - if: win
      then:
        - vc2019
      else:
        - gcc
    - ${{ compiler('cxx' )}}
  run:
    - ${{ "pywin32" if win }}

build:
  number: 0
"""

    recipe = Recipe.from_yaml(yaml_with_context)
    assert isinstance(recipe, SingleOutputRecipe)

    # Snapshot the context structure
    assert recipe.context == snapshot({"version": "1.2.3", "name": "my-package"})
    # Jinja should stay jinja in the stage0 recipe
    assert recipe.package.to_dict() == snapshot({"name": "${{ name }}", "version": "${{ version }}"})

    assert recipe.requirements.to_dict() == snapshot(
        {
            "build": [
                {"if": "win", "then": "vc2019", "else": "gcc"},
                "${{ compiler('cxx' )}}",
            ],
            "run": ['${{ "pywin32" if win }}'],
        }
    )
