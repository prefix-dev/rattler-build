import os
from pathlib import Path

import pytest
from helpers import RattlerBuild


def is_windows_arm64_host() -> bool:
    """Return whether Windows is natively running on ARM64.

    The E2E runner deliberately uses an emulated x64 rattler-build executable,
    so PROCESSOR_ARCHITEW6432 identifies the actual host in that case.
    """
    return os.name == "nt" and "ARM64" in {
        os.environ.get("PROCESSOR_ARCHITECTURE", "").upper(),
        os.environ.get("PROCESSOR_ARCHITEW6432", "").upper(),
    }


pytestmark = pytest.mark.skipif(
    not is_windows_arm64_host(), reason="requires a Windows ARM64 host"
)


@pytest.mark.parametrize(
    (
        "recipe",
        "platform",
        "architecture",
        "wow64_architecture",
        "process_architecture",
    ),
    [
        (
            "windows-architecture-execution",
            "win-arm64",
            "ARM64",
            "",
            "Arm64",
        ),
        (
            "windows-architecture-execution-x86",
            "win-32",
            "x86",
            "ARM64",
            "X86",
        ),
    ],
)
def test_windows_architecture_execution(
    rattler_build: RattlerBuild,
    recipes: Path,
    tmp_path: Path,
    recipe: str,
    platform: str,
    architecture: str,
    wow64_architecture: str,
    process_architecture: str,
):
    """An emulated x64 rattler-build launches scripts at the requested architecture."""
    result = rattler_build(
        "build",
        "--recipe",
        recipes / recipe,
        "--build-platform",
        platform,
        "--host-platform",
        platform,
        "--target-platform",
        platform,
        "--output-dir",
        tmp_path / platform,
        capture_output=True,
        text=True,
    )
    output = result.stdout + result.stderr

    # The recipes intentionally exit 37 after reporting their architecture.
    assert result.returncode != 0
    assert f"PROCESSOR_ARCHITECTURE={architecture}" in output
    assert f"PROCESSOR_ARCHITEW6432={wow64_architecture}" in output
    assert f"ProcessArchitecture={process_architecture}" in output
    assert "Script failed with status 37" in output
