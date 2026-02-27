"""Build the WASM playground and assemble the deploy directory."""

import shutil
import subprocess
from pathlib import Path

PLAYGROUND_CRATE = Path("crates/rattler_build_playground")
DEPLOY_DIR = Path("deploy")


def build_wasm() -> None:
    subprocess.run(
        ["wasm-pack", "build", "--target", "web", "--release", str(PLAYGROUND_CRATE)],
        check=True,
    )


def assemble_deploy() -> None:
    DEPLOY_DIR.mkdir(exist_ok=True)

    for f in (PLAYGROUND_CRATE / "www").iterdir():
        shutil.copy2(f, DEPLOY_DIR / f.name)

    pkg_dir = PLAYGROUND_CRATE / "pkg"
    shutil.copy2(pkg_dir / "rattler_build_playground_bg.wasm", DEPLOY_DIR)
    shutil.copy2(pkg_dir / "rattler_build_playground.js", DEPLOY_DIR)


if __name__ == "__main__":
    build_wasm()
    assemble_deploy()
