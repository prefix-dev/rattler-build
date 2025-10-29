"""Test ignore_keys functionality with context variables."""

from pathlib import Path
from helpers import RattlerBuild


def test_eigen_abi_profile_ignore_keys(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
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
        recipe_path, tmp_path, variant_config=variant_config
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
            eigen_outputs.append(
                {
                    "version": version,
                    "build_string": build_string,
                    "variant": variant,
                    "abi_profile": variant.get("eigen_abi_profile", "none"),
                    "some_key": variant.get("some_key", "none"),
                }
            )
        elif package_name == "eigen-abi":
            eigen_abi_outputs.append(
                {
                    "version": version,
                    "build_string": build_string,
                    "variant": variant,
                    "abi_profile": variant.get("eigen_abi_profile", "none"),
                    "some_key": variant.get("some_key", "none"),
                }
            )
        elif package_name == "eigen-abi-other":
            eigen_abi_other_outputs.append(
                {
                    "version": version,
                    "build_string": build_string,
                    "variant": variant,
                    "abi_profile": variant.get("eigen_abi_profile", "none"),
                    "some_key": variant.get("some_key", "none"),
                }
            )

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
    assert (
        len(eigen_abi_outputs) == 2
    ), f"Expected 2 eigen-abi outputs (one per abi_profile), got {len(eigen_abi_outputs)}"
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
        assert (
            "eigen_abi_profile" not in variant
            or variant.get("eigen_abi_profile") is None
        ), (
            f"eigen variant should not include eigen_abi_profile (it's ignored), "
            f"but variant is: {variant}"
        )
        # Should only have target_platform
        assert "target_platform" in variant, "eigen variant should have target_platform"
        assert (
            output["version"] == "3.4.0"
        ), f"eigen version should be 3.4.0, got {output['version']}"

    # Test 4: eigen-abi SHOULD show eigen_abi_profile in its variant (not ignored)
    abi_profiles_found = set()
    for output in eigen_abi_outputs:
        variant = output["variant"]
        # The variant dict SHOULD contain eigen_abi_profile for eigen-abi
        assert (
            "eigen_abi_profile" in variant
        ), f"eigen-abi variant should include eigen_abi_profile, but variant is: {variant}"
        abi_profile = variant["eigen_abi_profile"]
        abi_profiles_found.add(str(abi_profile))

        # Version should include the abi profile (e.g., 3.4.0.100)
        expected_version = f"3.4.0.{abi_profile}"
        assert (
            output["version"] == expected_version
        ), f"eigen-abi version should be {expected_version}, got {output['version']}"

    # Verify we saw both abi profiles
    assert abi_profiles_found == {
        "100",
        "80",
    }, f"Expected to find abi profiles 100 and 80, but found: {abi_profiles_found}"

    # eigen-abi should have different hashes
    eigen_abi_100 = next(
        (o for o in eigen_abi_outputs if o["abi_profile"] == "100"), None
    )
    eigen_abi_80 = next(
        (o for o in eigen_abi_outputs if o["abi_profile"] == "80"), None
    )

    assert (
        eigen_abi_100 is not None
    ), "Could not find eigen-abi output for abi_profile 100"
    assert (
        eigen_abi_80 is not None
    ), "Could not find eigen-abi output for abi_profile 80"

    hash_100 = eigen_abi_100["build_string"].split("_")[0]
    hash_80 = eigen_abi_80["build_string"].split("_")[0]

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
        assert (
            "some_key" in variant
        ), f"eigen-abi-other variant should include some_key (via use_keys), but variant is: {variant}"

        # Should NOT have eigen_abi_profile (via ignore_keys)
        assert (
            "eigen_abi_profile" not in variant
            or variant.get("eigen_abi_profile") is None
        ), (
            f"eigen-abi-other variant should not include eigen_abi_profile (it's ignored), "
            f"but variant is: {variant}"
        )

        some_key_value = variant["some_key"]
        some_keys_found.add(str(some_key_value))
        eigen_abi_other_by_some_key[str(some_key_value)] = output

    # Verify we saw both some_key values
    assert some_keys_found == {
        "1",
        "2",
    }, f"Expected to find some_key values 1 and 2, but found: {some_keys_found}"

    # Verify different some_key values produce different hashes
    hash_1 = eigen_abi_other_by_some_key["1"]["build_string"].split("_")[0]
    hash_2 = eigen_abi_other_by_some_key["2"]["build_string"].split("_")[0]

    assert hash_1 != hash_2, (
        f"eigen-abi-other outputs for different some_key values should have different hashes, "
        f"but both have: {hash_1}"
    )

    print("\n✓ Test passed!")
    print(f"  eigen (ignores abi_profile): {eigen_build_string}")
    print(f"  eigen-abi-100: {eigen_abi_100['build_string']}")
    print(f"  eigen-abi-80: {eigen_abi_80['build_string']}")
    print(
        f"  eigen-abi-other (some_key=1, ignores abi_profile): {eigen_abi_other_by_some_key['1']['build_string']}"
    )
    print(
        f"  eigen-abi-other (some_key=2, ignores abi_profile): {eigen_abi_other_by_some_key['2']['build_string']}"
    )
