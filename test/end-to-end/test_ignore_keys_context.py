"""Test ignore_keys functionality with context variables."""

from pathlib import Path
from helpers import RattlerBuild


def test_ignore_keys_with_context_variable(rattler_build: RattlerBuild, tmp_path: Path):
    """Test that ignore_keys properly excludes context variables from hash calculation."""
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    # Create a recipe that uses a context variable that should be ignored
    recipe_content = """
schema_version: 1

context:
  version: "1.0.0"
  my_variant: "100"

recipe:
  name: test-ignore-keys
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: test-output-a
    build:
      number: 0
      variant:
        ignore_keys:
          - my_variant

  - package:
      name: test-output-b
    build:
      number: 0
      variant:
        ignore_keys:
          - my_variant
"""

    (recipe_dir / "recipe.yaml").write_text(recipe_content)

    # Create a variant config with multiple values for my_variant
    variant_config_content = """
my_variant:
  - "100"
  - "200"
"""
    variant_config = recipe_dir / "conda_build_config.yaml"
    variant_config.write_text(variant_config_content)

    # Render the recipe to get variant information
    rendered = rattler_build.render(recipe_dir, tmp_path, variant_config=variant_config)

    # Collect build strings per package
    build_strings = {}
    for item in rendered:
        build_config = item.get("build_configuration", {})
        variant = build_config.get("variant", {})
        package_name = item["recipe"]["package"]["name"]
        build_string = item["recipe"]["build"]["string"]

        if package_name not in build_strings:
            build_strings[package_name] = []
        build_strings[package_name].append(build_string)

    # Both outputs should have the same build hash for all variants
    # because my_variant is ignored
    assert "test-output-a" in build_strings, "test-output-a was not rendered"
    assert "test-output-b" in build_strings, "test-output-b was not rendered"

    # Each output should have only one unique build string across both variants
    # since my_variant is ignored
    for package_name, strings in build_strings.items():
        unique_strings = set(strings)
        assert len(unique_strings) == 1, (
            f"Package {package_name} has different build strings {strings}, "
            f"but my_variant should be ignored in hash calculation (found {len(strings)} variants)"
        )

    # The hash component should be the same for both outputs
    output_a_hash = list(build_strings["test-output-a"])[0].rsplit('_', 1)[0]
    output_b_hash = list(build_strings["test-output-b"])[0].rsplit('_', 1)[0]

    assert output_a_hash == output_b_hash, (
        f"Outputs have different hashes: {output_a_hash} vs {output_b_hash}"
    )


def test_ignore_keys_with_dependency_and_context(rattler_build: RattlerBuild, tmp_path: Path):
    """Test that ignore_keys works with both dependency-based and context-based variants."""
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    # Create a recipe that uses both dependency and context variants
    recipe_content = """
schema_version: 1

context:
  version: "1.0.0"
  abi_profile: "100"

package:
  name: test-mixed-variants
  version: ${{ version }}

build:
  number: 0
  variant:
    use_keys:
      - python
    ignore_keys:
      - abi_profile

requirements:
  host:
    - python

about:
  summary: Test recipe with mixed variant sources
"""

    (recipe_dir / "recipe.yaml").write_text(recipe_content)

    # Create a variant config
    variant_config_content = """
python:
  - "3.11"
  - "3.12"
abi_profile:
  - "100"
  - "200"
"""
    variant_config = recipe_dir / "conda_build_config.yaml"
    variant_config.write_text(variant_config_content)

    # Render the recipe
    rendered = rattler_build.render(recipe_dir, tmp_path, variant_config=variant_config)

    # Group outputs by Python version
    python_variants = {}
    for item in rendered:
        build_config = item.get("build_configuration", {})
        variant = build_config.get("variant", {})
        python_version = variant.get("python", "unknown")
        build_string = item["recipe"]["build"]["string"]

        if python_version not in python_variants:
            python_variants[python_version] = []
        python_variants[python_version].append(build_string)

    # We should have 2 different Python variants (3.11 and 3.12)
    assert len(python_variants) == 2, (
        f"Expected 2 Python variants, got {len(python_variants)}: {list(python_variants.keys())}"
    )

    # For each Python version, all abi_profile values should produce the same hash
    for python_version, strings in python_variants.items():
        unique_strings = set(strings)
        assert len(unique_strings) == 1, (
            f"Python {python_version} has {len(strings)} builds with different hashes: {strings}, "
            "but abi_profile should be ignored and produce identical build strings"
        )


def test_eigen_abi_profile_ignore_keys(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test the eigen recipe pattern where eigen ignores eigen_abi_profile but eigen-abi includes it.

    This test validates the specific use case from the issue where:
    - The 'eigen' output should have the SAME hash across all eigen_abi_profile values (ignored)
    - The 'eigen-abi' output should have DIFFERENT hashes for each eigen_abi_profile value (not ignored)
    - The 'eigen-abi' package version should include the abi profile (e.g., 3.4.0.100)
    """
    recipe_path = recipes / "variants" / "issue_variant_ignore.yaml"
    variant_config = recipes / "variants" / "variant_config.yaml"

    # Render the recipe to get all variants
    rendered = rattler_build.render(
        recipe_path,
        tmp_path,
        variant_config=variant_config
    )

    # Separate the outputs by package name
    eigen_outputs = []
    eigen_abi_outputs = []
    eigen_abi_other_outputs = []

    for item in rendered:
        package_name = item["recipe"]["package"]["name"]
        build_config = item.get("build_configuration", {})
        variant = build_config.get("variant", {})
        build_string = item["recipe"]["build"]["string"]
        version = item["recipe"]["package"]["version"]

        if package_name == "eigen":
            eigen_outputs.append({
                "version": version,
                "build_string": build_string,
                "variant": variant,
                "abi_profile": variant.get("eigen_abi_profile", "none"),
                "some_key": variant.get("some_key", "none")
            })
        elif package_name == "eigen-abi":
            eigen_abi_outputs.append({
                "version": version,
                "build_string": build_string,
                "variant": variant,
                "abi_profile": variant.get("eigen_abi_profile", "none"),
                "some_key": variant.get("some_key", "none")
            })
        elif package_name == "eigen-abi-other":
            eigen_abi_other_outputs.append({
                "version": version,
                "build_string": build_string,
                "variant": variant,
                "abi_profile": variant.get("eigen_abi_profile", "none"),
                "some_key": variant.get("some_key", "none")
            })

    # Verify we have the expected number of outputs
    # eigen: ignores eigen_abi_profile -> 1 build (same hash for all abi profiles)
    # eigen-abi: does NOT ignore eigen_abi_profile -> 2 builds (one per abi profile: 100, 80)
    # eigen-abi-other: uses use_keys: [some_key] and ignore_keys: [eigen_abi_profile]
    #   use_keys forces some_key into the variant -> 2 builds (one per some_key: 1, 2)
    #   ignore_keys excludes eigen_abi_profile from hash
    assert len(eigen_outputs) == 1, (
        f"Expected 1 eigen output (since ignore_keys makes all abi_profile variants identical), "
        f"got {len(eigen_outputs)}"
    )
    assert len(eigen_abi_outputs) == 2, (
        f"Expected 2 eigen-abi outputs (one per abi_profile), got {len(eigen_abi_outputs)}"
    )
    assert len(eigen_abi_other_outputs) == 2, (
        f"Expected 2 eigen-abi-other outputs (one per some_key via use_keys), "
        f"got {len(eigen_abi_other_outputs)}"
    )

    # Test 1: eigen output should only be built once (all abi profiles produce same hash)
    # This is the whole point of ignore_keys!
    eigen_build_string = eigen_outputs[0]["build_string"]

    # Test 2: eigen-abi output should have DIFFERENT build strings for each abi profile
    eigen_abi_build_strings = [o["build_string"] for o in eigen_abi_outputs]
    unique_eigen_abi_builds = set(eigen_abi_build_strings)
    assert len(unique_eigen_abi_builds) == 2, (
        f"eigen-abi package should have different build strings for each abi_profile value, "
        f"but got: {eigen_abi_build_strings}"
    )

    # Test 3: Verify the variant information displayed
    # eigen should NOT show eigen_abi_profile in its variant (it's ignored)
    for output in eigen_outputs:
        variant = output["variant"]
        # The variant dict should NOT contain eigen_abi_profile for the eigen package
        # because it's in ignore_keys
        assert "eigen_abi_profile" not in variant or variant.get("eigen_abi_profile") is None, (
            f"eigen variant should not include eigen_abi_profile (it's ignored), "
            f"but variant is: {variant}"
        )
        # Should only have target_platform
        assert "target_platform" in variant, "eigen variant should have target_platform"
        assert output["version"] == "3.4.0", f"eigen version should be 3.4.0, got {output['version']}"

    # Test 4: eigen-abi SHOULD show eigen_abi_profile in its variant (not ignored)
    abi_profiles_found = set()
    for output in eigen_abi_outputs:
        variant = output["variant"]
        # The variant dict SHOULD contain eigen_abi_profile for eigen-abi
        assert "eigen_abi_profile" in variant, (
            f"eigen-abi variant should include eigen_abi_profile, but variant is: {variant}"
        )
        abi_profile = variant["eigen_abi_profile"]
        abi_profiles_found.add(str(abi_profile))

        # Version should include the abi profile (e.g., 3.4.0.100)
        expected_version = f"3.4.0.{abi_profile}"
        assert output["version"] == expected_version, (
            f"eigen-abi version should be {expected_version}, got {output['version']}"
        )

    # Verify we saw both abi profiles
    assert abi_profiles_found == {"100", "80"}, (
        f"Expected to find abi profiles 100 and 80, but found: {abi_profiles_found}"
    )

    # Test 5: Verify the expected hash patterns from the issue
    # eigen should have hash h60d57d3 (same for all abi profiles)
    eigen_hash = eigen_build_string.split('_')[0]
    assert eigen_hash == "h60d57d3", (
        f"eigen hash should be h60d57d3 (consistent across all abi profiles), got {eigen_hash}"
    )

    # eigen-abi should have different hashes
    eigen_abi_100 = next((o for o in eigen_abi_outputs if o["abi_profile"] == "100"), None)
    eigen_abi_80 = next((o for o in eigen_abi_outputs if o["abi_profile"] == "80"), None)

    assert eigen_abi_100 is not None, "Could not find eigen-abi output for abi_profile 100"
    assert eigen_abi_80 is not None, "Could not find eigen-abi output for abi_profile 80"

    hash_100 = eigen_abi_100["build_string"].split('_')[0]
    hash_80 = eigen_abi_80["build_string"].split('_')[0]

    # Verify they have different hashes (the actual hash values may vary)
    assert hash_100 != hash_80, (
        f"eigen-abi outputs for different abi_profiles should have different hashes, "
        f"but both have: {hash_100}"
    )

    # Test 6: eigen-abi-other validates that use_keys and ignore_keys work together
    # use_keys forces some_key into variant even though it's not referenced
    # ignore_keys prevents eigen_abi_profile from affecting the hash
    some_keys_found = set()
    eigen_abi_other_by_some_key = {}

    for output in eigen_abi_other_outputs:
        variant = output["variant"]

        # Should have some_key (via use_keys)
        assert "some_key" in variant, (
            f"eigen-abi-other variant should include some_key (via use_keys), but variant is: {variant}"
        )

        # Should NOT have eigen_abi_profile (via ignore_keys)
        assert "eigen_abi_profile" not in variant or variant.get("eigen_abi_profile") is None, (
            f"eigen-abi-other variant should not include eigen_abi_profile (it's ignored), "
            f"but variant is: {variant}"
        )

        some_key_value = variant["some_key"]
        some_keys_found.add(str(some_key_value))
        eigen_abi_other_by_some_key[str(some_key_value)] = output

    # Verify we saw both some_key values
    assert some_keys_found == {"1", "2"}, (
        f"Expected to find some_key values 1 and 2, but found: {some_keys_found}"
    )

    # Verify different some_key values produce different hashes
    hash_1 = eigen_abi_other_by_some_key["1"]["build_string"].split('_')[0]
    hash_2 = eigen_abi_other_by_some_key["2"]["build_string"].split('_')[0]

    assert hash_1 != hash_2, (
        f"eigen-abi-other outputs for different some_key values should have different hashes, "
        f"but both have: {hash_1}"
    )

    print("\n✓ Test passed!")
    print(f"  eigen (ignores abi_profile): {eigen_build_string}")
    print(f"  eigen-abi-100: {eigen_abi_100['build_string']}")
    print(f"  eigen-abi-80: {eigen_abi_80['build_string']}")
    print(f"  eigen-abi-other (some_key=1, ignores abi_profile): {eigen_abi_other_by_some_key['1']['build_string']}")
    print(f"  eigen-abi-other (some_key=2, ignores abi_profile): {eigen_abi_other_by_some_key['2']['build_string']}")
