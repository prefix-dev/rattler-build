"""Serve the playground with live reload.

Watches the www/ source files and pkg/ WASM artifacts, syncs changes
into deploy/, and auto-reloads the browser.
"""

import shutil
from pathlib import Path

from livereload import Server

PLAYGROUND_CRATE = Path("crates/rattler_build_playground")
WWW_DIR = PLAYGROUND_CRATE / "www"
PKG_DIR = PLAYGROUND_CRATE / "pkg"
DEPLOY_DIR = Path("deploy")

WASM_FILES = [
    "rattler_build_playground_bg.wasm",
    "rattler_build_playground.js",
]


def sync_www() -> None:
    """Copy all www/ files into deploy/."""
    for f in WWW_DIR.iterdir():
        if f.is_file():
            shutil.copy2(f, DEPLOY_DIR / f.name)


def sync_pkg() -> None:
    """Copy WASM artifacts from pkg/ into deploy/."""
    for name in WASM_FILES:
        src = PKG_DIR / name
        if src.exists():
            shutil.copy2(src, DEPLOY_DIR / name)


if __name__ == "__main__":
    DEPLOY_DIR.mkdir(exist_ok=True)
    sync_www()
    sync_pkg()

    server = Server()
    server.watch(str(WWW_DIR) + "/**", sync_www)
    server.watch(str(PKG_DIR) + "/*.wasm", sync_pkg)
    server.watch(str(PKG_DIR) + "/*.js", sync_pkg)
    server.serve(root=str(DEPLOY_DIR), port=8080)
