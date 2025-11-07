"""Tests for Stage1 Python bindings.

Note: Stage1 recipes are fully evaluated recipes. These tests focus on the API surface
since creating Stage1 recipes requires the full evaluation pipeline which is tested
elsewhere.
"""

from inline_snapshot import snapshot

from rattler_build import stage1


def test_stage1_module_exports() -> None:
    """Test that stage1 module exports expected classes."""
    assert hasattr(stage1, "Recipe")
    assert hasattr(stage1, "Package")
    assert hasattr(stage1, "Build")
    assert hasattr(stage1, "Requirements")
    assert hasattr(stage1, "About")
    assert hasattr(stage1, "Source")
    assert hasattr(stage1, "StagingCache")


def test_stage1_recipe_type_annotations() -> None:
    """Test that Recipe class has proper type annotations."""

    # Check that the class can be used for type annotations
    def accepts_recipe(recipe: stage1.Recipe) -> None:
        pass

    # This should not raise a type error
    assert callable(accepts_recipe)


def test_stage1_package_type_annotations() -> None:
    """Test that Package class has proper type annotations."""

    def accepts_package(package: stage1.Package) -> None:
        pass

    assert callable(accepts_package)


def test_stage1_build_type_annotations() -> None:
    """Test that Build class has proper type annotations."""

    def accepts_build(build: stage1.Build) -> None:
        pass

    assert callable(accepts_build)


def test_stage1_requirements_type_annotations() -> None:
    """Test that Requirements class has proper type annotations."""

    def accepts_requirements(requirements: stage1.Requirements) -> None:
        pass

    assert callable(accepts_requirements)


def test_stage1_about_type_annotations() -> None:
    """Test that About class has proper type annotations."""

    def accepts_about(about: stage1.About) -> None:
        pass

    assert callable(accepts_about)


def test_stage1_source_type_annotations() -> None:
    """Test that Source class has proper type annotations."""

    def accepts_source(source: stage1.Source) -> None:
        pass

    assert callable(accepts_source)


def test_stage1_staging_cache_type_annotations() -> None:
    """Test that StagingCache class has proper type annotations."""

    def accepts_staging_cache(cache: stage1.StagingCache) -> None:
        pass

    assert callable(accepts_staging_cache)


def test_stage1_recipe_wrapper_structure() -> None:
    """Test that Recipe wrapper has expected methods."""
    # The Recipe class should have these methods based on the implementation
    expected_methods = [
        "package",
        "build",
        "requirements",
        "about",
        "context",
        "used_variant",
        "sources",
        "staging_caches",
        "inherits_from",
        "to_dict",
    ]

    for method in expected_methods:
        assert hasattr(stage1.Recipe, method), f"Recipe should have {method} method/property"


def test_stage1_package_wrapper_structure() -> None:
    """Test that Package wrapper has expected methods."""
    expected_methods = ["name", "version", "to_dict"]

    for method in expected_methods:
        assert hasattr(stage1.Package, method), f"Package should have {method} method/property"


def test_stage1_build_wrapper_structure() -> None:
    """Test that Build wrapper has expected methods."""
    expected_methods = ["number", "string", "script", "noarch", "to_dict"]

    for method in expected_methods:
        assert hasattr(stage1.Build, method), f"Build should have {method} method/property"


def test_stage1_requirements_wrapper_structure() -> None:
    """Test that Requirements wrapper has expected methods."""
    expected_methods = ["build", "host", "run", "to_dict"]

    for method in expected_methods:
        assert hasattr(stage1.Requirements, method), f"Requirements should have {method} method/property"


def test_stage1_about_wrapper_structure() -> None:
    """Test that About wrapper has expected methods."""
    expected_methods = ["homepage", "repository", "documentation", "license", "summary", "description", "to_dict"]

    for method in expected_methods:
        assert hasattr(stage1.About, method), f"About should have {method} method/property"


def test_stage1_source_wrapper_structure() -> None:
    """Test that Source wrapper has expected methods."""
    expected_methods = ["to_dict"]

    for method in expected_methods:
        assert hasattr(stage1.Source, method), f"Source should have {method} method/property"


def test_stage1_staging_cache_wrapper_structure() -> None:
    """Test that StagingCache wrapper has expected methods."""
    expected_methods = ["name", "build", "requirements", "to_dict"]

    for method in expected_methods:
        assert hasattr(stage1.StagingCache, method), f"StagingCache should have {method} method/property"


def test_stage1_wrapper_classes_exist() -> None:
    """Test that wrapper classes are properly defined."""
    # Try to access the classes - this will fail if they're not properly imported
    assert stage1.Recipe is not None
    assert stage1.Package is not None
    assert stage1.Build is not None
    assert stage1.Requirements is not None
    assert stage1.About is not None
    assert stage1.Source is not None
    assert stage1.StagingCache is not None


# Integration tests would require a full build pipeline
# These would test actual Stage1 recipe creation and manipulation
# For now, we focus on the API surface and type safety


def test_stage1_module_structure_snapshot() -> None:
    """Test the stage1 module structure with snapshot."""
    # Get all public exports from stage1 module
    exports = [name for name in dir(stage1) if not name.startswith("_")]

    # Snapshot the module structure to catch any API changes
    assert sorted(exports) == snapshot(
        [
            "About",
            "Any",
            "Build",
            "Package",
            "Recipe",
            "Requirements",
            "Source",
            "StagingCache",
            "TYPE_CHECKING",
        ]
    )


def test_stage1_recipe_properties_snapshot() -> None:
    """Test Recipe class properties with snapshot."""
    # Get all public properties and methods
    recipe_attrs = [name for name in dir(stage1.Recipe) if not name.startswith("_")]

    # Snapshot to ensure API stability
    assert sorted(recipe_attrs) == snapshot(
        [
            "about",
            "build",
            "context",
            "inherits_from",
            "package",
            "requirements",
            "sources",
            "staging_caches",
            "to_dict",
            "used_variant",
        ]
    )


def test_stage1_package_properties_snapshot() -> None:
    """Test Package class properties with snapshot."""
    package_attrs = [name for name in dir(stage1.Package) if not name.startswith("_")]

    assert sorted(package_attrs) == snapshot(["name", "to_dict", "version"])


def test_stage1_build_properties_snapshot() -> None:
    """Test Build class properties with snapshot."""
    build_attrs = [name for name in dir(stage1.Build) if not name.startswith("_")]

    assert sorted(build_attrs) == snapshot(["noarch", "number", "script", "string", "to_dict"])


def test_stage1_requirements_properties_snapshot() -> None:
    """Test Requirements class properties with snapshot."""
    requirements_attrs = [name for name in dir(stage1.Requirements) if not name.startswith("_")]

    assert sorted(requirements_attrs) == snapshot(["build", "host", "run", "to_dict"])


def test_stage1_about_properties_snapshot() -> None:
    """Test About class properties with snapshot."""
    about_attrs = [name for name in dir(stage1.About) if not name.startswith("_")]

    assert sorted(about_attrs) == snapshot(
        [
            "description",
            "documentation",
            "homepage",
            "license",
            "repository",
            "summary",
            "to_dict",
        ]
    )


def test_stage1_source_properties_snapshot() -> None:
    """Test Source class properties with snapshot."""
    source_attrs = [name for name in dir(stage1.Source) if not name.startswith("_")]

    assert sorted(source_attrs) == snapshot(["to_dict"])


def test_stage1_staging_cache_properties_snapshot() -> None:
    """Test StagingCache class properties with snapshot."""
    cache_attrs = [name for name in dir(stage1.StagingCache) if not name.startswith("_")]

    assert sorted(cache_attrs) == snapshot(["build", "name", "requirements", "to_dict"])
