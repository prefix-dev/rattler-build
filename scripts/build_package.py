import subprocess
from datetime import datetime, timezone
from pathlib import Path

import tomlkit


def get_version_from_cargo() -> str:
    content = Path("Cargo.toml").read_text()
    version = tomlkit.loads(content)["package"]["version"]
    assert isinstance(version, str)
    return version


def get_git_short_hash() -> str:
    return subprocess.run(
        ["git", "rev-parse", "--short=7", "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def set_pixi_package_version(version: str) -> None:
    pixi_toml = Path("pixi.toml")
    doc = tomlkit.loads(pixi_toml.read_text())
    doc["package"]["version"] = version
    pixi_toml.write_text(tomlkit.dumps(doc))


def build() -> None:
    version = get_version_from_cargo()
    now = datetime.now(tz=timezone.utc)
    timestamp = now.strftime("%Y%m%d")
    time = now.strftime("%H%M")
    short_hash = get_git_short_hash()
    rattler_build_version = f"{version}.{timestamp}.{time}.{short_hash}"

    print(f"Building rattler-build {rattler_build_version}")

    set_pixi_package_version(rattler_build_version)

    subprocess.run(["pixi", "build", "--verbose"], check=True)
    print("Build completed successfully")


if __name__ == "__main__":
    build()
