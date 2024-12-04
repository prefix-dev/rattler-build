from pathlib import Path

import rattler_build


def test_basic() -> None:
    parent_cargo_toml = Path(__file__).parent.parent.parent.parent / "Cargo.toml"
    # get the version with a regex
    text = parent_cargo_toml.read_text()
    for line in text.splitlines():
        if line.startswith("version"):
            version = line.split("=")[1].strip().strip('"')
            break
    assert rattler_build.rattler_build_version() == version
