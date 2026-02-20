import subprocess
from pathlib import Path


def find_package() -> Path:
    packages = list(Path(".").glob("rattler-build-*.conda"))
    if len(packages) != 1:
        raise RuntimeError(
            f"expected exactly one rattler-build package, found {len(packages)}: {packages}"
        )
    return packages[0]


def main() -> None:
    package = find_package()
    print(f"Uploading {package}")
    subprocess.run(
        [
            "pixi",
            "upload",
            "prefix",
            "--channel",
            "rattler-build-pre-release",
            str(package),
        ],
        check=True,
    )
    print(f"Successfully uploaded {package}")


if __name__ == "__main__":
    main()
