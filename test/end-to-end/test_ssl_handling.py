import pytest
from pathlib import Path
from helpers import RattlerBuild


def test_insecure_ssl_failure(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, capfd
):
    recipe_path = recipes / "ssl_test"
    output_path = tmp_path / "output"

    # build should fail because of ssl certificate verification failure
    with pytest.raises(Exception):
        rattler_build.build(recipe_path, output_path)

    captured = capfd.readouterr()
    output = captured.out + captured.err
    # Check that download failed (the retry middleware may hide the specific SSL error)
    assert "Failed to download from URL" in output or "Failed to fetch" in output


def test_insecure_ssl_success(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, capfd
):
    recipe_path = recipes / "ssl_test"
    output_path = tmp_path / "output"

    insecure_hosts = ["untrusted-root.badssl.com", "self-signed.badssl.com"]

    extra_args = []
    for host in insecure_hosts:
        extra_args.extend(["--allow-insecure-host", host])

    # this should now succeed with the insecure hosts allowed
    rattler_build.build(recipe_path, output_path, extra_args=extra_args)

    captured = capfd.readouterr()
    output = captured.out + captured.err
    # Check that the build succeeded (no download failures)
    assert "Failed to download from URL" not in output
    assert "Failed to fetch" not in output


def test_specific_insecure_host(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, capfd
):
    recipe_path = recipes / "ssl_test"
    output_path = tmp_path / "output"

    # only allow one of the problematic hosts
    extra_args = ["--allow-insecure-host", "untrusted-root.badssl.com"]

    # should still fail because self-signed.badssl.com is not in the allowed list
    with pytest.raises(Exception):
        rattler_build.build(recipe_path, output_path, extra_args=extra_args)

    captured = capfd.readouterr()
    output = captured.out + captured.err
    # Check that download failed (the retry middleware may hide the specific SSL error)
    assert "Failed to download from URL" in output or "Failed to fetch" in output


def test_url_handling(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, capfd
):
    recipe_path = recipes / "ssl_test"
    output_path = tmp_path / "output"

    extra_args = [
        "--allow-insecure-host",
        "untrusted-root.badssl.com",
        "--allow-insecure-host",
        "self-signed.badssl.com",
    ]

    # Build should succeed with both hosts allowed
    rattler_build.build(recipe_path, output_path, extra_args=extra_args)

    captured = capfd.readouterr()
    output = captured.out + captured.err
    # Check that the build succeeded (no download failures)
    assert "Failed to download from URL" not in output
    assert "Failed to fetch" not in output
