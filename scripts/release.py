"""Interactive release script: bump version and update all lock files.

Usage:
    pixi run release
"""

import atexit
import os
import re
import subprocess
import sys
import tomllib
from enum import Enum
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

VERSION_FILES: list[tuple[Path, list[str]]] = [
    (ROOT / "Cargo.toml", ["package", "version"]),
    (ROOT / "py-rattler-build/rust/Cargo.toml", ["package", "version"]),
    (ROOT / "py-rattler-build/pyproject.toml", ["project", "version"]),
]

VERSION_PATTERN = re.compile(r"^\d+\.\d+\.\d+$")

STEPS = [
    "Pre-flight checks",
    "Patch version files with tbump",
    "Verify version files are in sync",
    "Update changelog",
    "Update Cargo.lock",
    "Update pixi.lock (root)",
    "Update pixi.lock (py-rattler-build/)",
]

completed: list[str] = []


class Color(str, Enum):
    YELLOW = "\033[93m"
    MAGENTA = "\033[95m"
    RESET = "\033[0m"


def cprint(msg: str, color: Color = Color.YELLOW) -> None:
    print(f"{color.value}{msg}{Color.RESET.value}")


def cinput(prompt: str, color: Color = Color.MAGENTA) -> str:
    return input(f"{color.value}{prompt}{Color.RESET.value}")


def run(
    cmd: list[str], *, cwd: Path = ROOT, capture: bool = False
) -> subprocess.CompletedProcess[str]:
    cprint(f"  → {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, cwd=cwd, text=True, capture_output=capture)


def fail(msg: str) -> None:
    print(f"\nError: {msg}", file=sys.stderr)
    sys.exit(1)


def print_summary() -> None:
    if completed:
        cprint("\nCompleted steps:")
        for step in completed:
            cprint(f"  - {step}")


atexit.register(print_summary)


# --- Pre-flight checks ---


def check_clean_worktree() -> None:
    """Ensure there are no uncommitted changes."""
    result = run(["git", "status", "--porcelain"], capture=True)
    if result.stdout.strip():
        fail("working directory is not clean. Commit or stash your changes first.")


def check_on_main() -> None:
    """Ensure we're on the main branch."""
    result = run(["git", "rev-parse", "--abbrev-ref", "HEAD"], capture=True)
    branch = result.stdout.strip()
    # jj may report HEAD for detached state; also check via symbolic-ref
    if branch == "HEAD":
        # In jj colocated repos, HEAD is often an empty commit on top of main.
        # Check that main is an ancestor of HEAD (i.e. HEAD is at or just above main).
        result = subprocess.run(
            ["git", "merge-base", "--is-ancestor", "main", "HEAD"],
            cwd=ROOT,
        )
        if result.returncode != 0:
            main_rev = run(["git", "rev-parse", "main"], capture=True).stdout.strip()
            head_rev = run(["git", "rev-parse", "HEAD"], capture=True).stdout.strip()
            fail(
                f"not on main (HEAD={head_rev[:12]}, main={main_rev[:12]}). Switch to main first."
            )
    elif branch != "main":
        fail(f"not on main branch (currently on '{branch}'). Switch to main first.")


def check_no_existing_tag(version: str) -> None:
    """Ensure the target tag doesn't already exist."""
    tag = f"v{version}"
    result = subprocess.run(
        ["git", "tag", "--list", tag],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if tag in result.stdout.strip().splitlines():
        fail(
            f"tag '{tag}' already exists. Choose a different version or delete the existing tag."
        )


# --- Version helpers ---


def get_current_version() -> str | None:
    """Read the current version from the root Cargo.toml."""
    path, keys = VERSION_FILES[0]
    data = tomllib.loads(path.read_text())
    value = data
    for key in keys:
        value = value.get(key)
        if value is None:
            return None
    return value


def get_release_version() -> str:
    """Interactively prompt for the release version."""
    current = get_current_version()
    while True:
        prompt = (
            f"Enter the release version (X.Y.Z) [{current}]: "
            if current
            else "Enter the release version (X.Y.Z): "
        )
        version = cinput(prompt).strip() or (current or "")
        if VERSION_PATTERN.match(version):
            return version
        cprint("Invalid format. Please enter the version as X.Y.Z (e.g. 0.59.0).")


def check_versions_in_sync(expected: str) -> None:
    """Verify all version files contain the expected version."""
    mismatches: list[str] = []
    for path, keys in VERSION_FILES:
        data = tomllib.loads(path.read_text())
        version = data
        for key in keys:
            version = version.get(key)
            if version is None:
                break
        if version is None:
            mismatches.append(f"  {path.relative_to(ROOT)}: version not found")
        elif version != expected:
            mismatches.append(
                f"  {path.relative_to(ROOT)}: {version} (expected {expected})"
            )
    if mismatches:
        fail("version files are out of sync after bump:\n" + "\n".join(mismatches))


# --- Main ---


def select_start_step() -> int:
    """Prompt the user to select which step to start from."""
    cprint("Select the step to start from:")
    for i, step in enumerate(STEPS, 1):
        cprint(f"  {i}. {step}")
    while True:
        try:
            choice = int(cinput("Enter the step number: "))
            if 1 <= choice <= len(STEPS):
                return choice
            cprint(f"Please enter a number between 1 and {len(STEPS)}.")
        except ValueError:
            cprint("Invalid input. Please enter a number.")


def main() -> None:
    start_step = select_start_step()
    version = get_release_version()

    cprint(f"\n=== Releasing {version} ===\n")

    try:
        if start_step <= 1:
            cprint("1. Running pre-flight checks...")
            check_clean_worktree()
            check_on_main()
            check_no_existing_tag(version)
            cprint("  All checks passed.")
            completed.append("Pre-flight checks")

        if start_step <= 2:
            cprint("\n2. Patching version files with tbump...")
            run(["tbump", "--non-interactive", "--only-patch", version])
            completed.append("Patched version files")

        if start_step <= 3:
            cprint("\n3. Verifying version files are in sync...")
            check_versions_in_sync(version)
            cprint("  All version files match.")
            completed.append("Verified version files")

        if start_step <= 4:
            cprint("\n4. Updating changelog...")
            env = {**os.environ, "RELEASE_VERSION": version}
            cprint("  → pixi run bump-changelog")
            subprocess.run(
                ["pixi", "run", "bump-changelog"], check=True, cwd=ROOT, env=env
            )
            cinput(
                "Update the 'Highlights' section in CHANGELOG.md, then press Enter to continue..."
            )
            completed.append("Updated changelog")

        if start_step <= 5:
            cprint("\n5. Updating Cargo.lock...")
            run(["cargo", "update", "--workspace"])
            completed.append("Updated Cargo.lock")

        if start_step <= 6:
            cprint("\n6. Updating pixi.lock (root)...")
            run(["pixi", "lock"])
            completed.append("Updated pixi.lock (root)")

        if start_step <= 7:
            cprint("\n7. Updating pixi.lock (py-rattler-build/)...")
            run(["pixi", "lock"], cwd=ROOT / "py-rattler-build")
            completed.append("Updated pixi.lock (py-rattler-build/)")

        cprint("\n=== Done ===")
        cprint(f"Version bumped to {version}.")
        cprint("Review the changes and open a PR.")
        cprint("After merge, trigger the Release workflow via workflow_dispatch.")

    except KeyboardInterrupt:
        cprint("\nInterrupted.")


if __name__ == "__main__":
    main()
