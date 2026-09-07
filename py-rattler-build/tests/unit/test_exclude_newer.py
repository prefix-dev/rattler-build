"""Cutoff policies resolve real packages from an offline channel."""

from __future__ import annotations

import hashlib
import io
import json
import subprocess
import sys
import tarfile
from base64 import b64encode
from collections.abc import Iterator
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Literal
from urllib.parse import urlsplit
from urllib.request import url2pathname

import pytest

from rattler_build import Package, Stage0Recipe, ToolConfiguration
from rattler_build.debug import DebugSession
from rattler_build.render import build_rendered_variants

CUTOFF = datetime(2024, 1, 1, tzinfo=timezone.utc)
OLD = datetime(2020, 1, 1, tzinfo=timezone.utc)
NEW = datetime(2030, 1, 1, tzinfo=timezone.utc)


@pytest.fixture
def cutoff_channel(tmp_path: Path) -> str:
    channel = tmp_path / "channel"
    records = {}
    for name in ("cutoff-dependency", "cutoff-other", "cutoff-unknown"):
        for version, timestamp in (("1", OLD), ("2", NEW)):
            if name == "cutoff-unknown" and version == "2":
                continue
            index: dict[str, Any] = {
                "name": name,
                "version": version,
                "build": "0",
                "build_number": 0,
                "subdir": "noarch",
                "noarch": "generic",
                "depends": [],
            }
            if name != "cutoff-unknown":
                index["timestamp"] = int(timestamp.timestamp() * 1000)
            marker = f"share/{name}.txt"
            content = f"{version}\n".encode()
            paths = {
                "paths_version": 1,
                "paths": [
                    {
                        "_path": marker,
                        "path_type": "hardlink",
                        "sha256": hashlib.sha256(content).hexdigest(),
                        "size_in_bytes": len(content),
                    }
                ],
            }
            package_path = channel / "noarch" / f"{name}-{version}-0.tar.bz2"
            package_path.parent.mkdir(parents=True, exist_ok=True)
            with tarfile.open(package_path, "w:bz2") as archive:
                for filename, data in {
                    "info/index.json": json.dumps(index).encode(),
                    "info/paths.json": json.dumps(paths).encode(),
                    "info/files": f"{marker}\n".encode(),
                    marker: content,
                }.items():
                    entry = tarfile.TarInfo(filename)
                    entry.size = len(data)
                    entry.mode = 0o644
                    archive.addfile(entry, io.BytesIO(data))
            records[package_path.name] = {
                **index,
                "sha256": hashlib.sha256(package_path.read_bytes()).hexdigest(),
                "size": package_path.stat().st_size,
            }
    for subdir in (
        "noarch",
        "linux-64",
        "linux-aarch64",
        "linux-ppc64le",
        "linux-s390x",
        "linux-armv7l",
        "linux-riscv64",
        "osx-64",
        "osx-arm64",
        "win-64",
        "win-arm64",
    ):
        destination = channel / subdir
        destination.mkdir(exist_ok=True)
        (destination / "repodata.json").write_text(
            json.dumps(
                {
                    "info": {"subdir": subdir},
                    "repodata_version": 1,
                    "packages": records if subdir == "noarch" else {},
                    "packages.conda": {},
                }
            )
        )
    return channel.as_uri() + "/"


@pytest.fixture
def cutoff_http_channel(cutoff_channel: str, tmp_path: Path) -> Iterator[tuple[str, Path]]:
    """Serve the tiny channel independently of the native binding's Python GIL."""
    requests_path = tmp_path / "requests.jsonl"
    requests_path.touch()
    server_code = """
import json
import sys
from functools import partial
from http.server import HTTPServer, SimpleHTTPRequestHandler

class Handler(SimpleHTTPRequestHandler):
    def do_GET(self):
        with open(sys.argv[2], "a") as requests:
            requests.write(json.dumps({
                "path": self.path,
                "authorization": self.headers.get("Authorization"),
            }) + "\\n")
        if self.path.startswith("/t/cutoff-test-token/"):
            self.path = self.path.removeprefix("/t/cutoff-test-token")
        super().do_GET()

    def log_message(self, *args):
        pass

with HTTPServer(("127.0.0.1", 0), partial(Handler, directory=sys.argv[1])) as server:
    print(server.server_address[1], flush=True)
    server.serve_forever()
"""
    with subprocess.Popen(
        [sys.executable, "-u", "-c", server_code, url2pathname(urlsplit(cutoff_channel).path), str(requests_path)],
        stdout=subprocess.PIPE,
        text=True,
    ) as server:
        assert server.stdout is not None
        port = int(server.stdout.readline())
        try:
            yield f"http://127.0.0.1:{port}", requests_path
        finally:
            server.terminate()
            server.wait(timeout=5)


def dependency_recipe() -> Stage0Recipe:
    """Assert selected dependencies in both build prefixes and native tests."""
    return Stage0Recipe.from_yaml(
        """
package:
  name: cutoff-result
  version: "1"
build:
  noarch: generic
  script:
    - if: unix
      then:
        - test "$(cat "$BUILD_PREFIX/share/cutoff-dependency.txt")" = "1"
        - test "$(cat "$PREFIX/share/cutoff-other.txt")" = "1"
        - echo built > "$PREFIX/cutoff-result.txt"
      else:
        - findstr /x "1" "%BUILD_PREFIX%\\share\\cutoff-dependency.txt"
        - findstr /x "1" "%PREFIX%\\share\\cutoff-other.txt"
        - echo built > "%PREFIX%\\cutoff-result.txt"
requirements:
  build:
    - cutoff-dependency
  host:
    - cutoff-other
  run:
    - cutoff-other
tests:
  - requirements:
      run:
        - cutoff-dependency
    script:
      - if: unix
        then:
          - test "$(cat "$PREFIX/share/cutoff-dependency.txt")" = "1"
          - test "$(cat "$PREFIX/share/cutoff-other.txt")" = "1"
          - test -f "$PREFIX/cutoff-result.txt"
        else:
          - findstr /x "1" "%PREFIX%\\share\\cutoff-dependency.txt"
          - findstr /x "1" "%PREFIX%\\share\\cutoff-other.txt"
          - if not exist "%PREFIX%\\cutoff-result.txt" exit 1
"""
    )


@pytest.mark.parametrize("entrypoint", ["recipe", "variant", "variants"])
def test_build_and_test_cutoff(cutoff_channel: str, tmp_path: Path, entrypoint: str) -> None:
    recipe = dependency_recipe()
    options: dict[str, Any] = {
        "output_dir": tmp_path / "output",
        "channels": [cutoff_channel],
        "tool_config": ToolConfiguration(test_strategy="native"),
    }
    if entrypoint == "recipe":
        options["exclude_newer_package"] = {"cutoff-dependency": CUTOFF, "cutoff-other": CUTOFF}
        result = recipe.run_build(**options)[0]
    elif entrypoint == "variant":
        options["exclude_newer"] = CUTOFF
        result = recipe.render()[0].run_build(**options)
    else:
        options["exclude_newer_channel"] = {cutoff_channel: CUTOFF}
        result = build_rendered_variants(recipe.render(), **options)[0]

    package = Package.from_file(result.packages[0])
    assert package.timestamp is not None and package.timestamp > CUTOFF
    tests = package.run_tests(channel=[cutoff_channel], exclude_newer=CUTOFF)
    assert len(tests) == 1 and tests[0].success
    assert package.run_test(
        0,
        channel=[cutoff_channel],
        exclude_newer_package={"cutoff-dependency": CUTOFF, "cutoff-other": CUTOFF},
    ).success
    if entrypoint == "variant":
        # Explicit package cutoffs take precedence over the test channel exemption.
        blocked = package.run_tests(
            channel=[cutoff_channel],
            exclude_newer=CUTOFF,
            exclude_newer_package={"cutoff-result": CUTOFF},
        )
        assert len(blocked) == 1 and not blocked[0].success


@pytest.mark.parametrize(
    ("global_cutoff", "package_cutoffs", "channel_cutoff", "expected"),
    [
        (CUTOFF, None, "unset", ("1", "1")),
        (None, {"cutoff-dependency": CUTOFF}, "unset", ("1", "2")),
        (CUTOFF, {"cutoff-dependency": None}, "unset", ("2", "1")),
        (None, None, CUTOFF, ("1", "1")),
        (CUTOFF, None, None, ("2", "2")),
        (CUTOFF, {"cutoff-dependency": CUTOFF}, None, ("1", "2")),
        (CUTOFF, {"cutoff-dependency": None}, CUTOFF, ("2", "1")),
    ],
)
def test_debug_cutoff_overrides(
    cutoff_channel: str,
    tmp_path: Path,
    global_cutoff: datetime | None,
    package_cutoffs: dict[str, datetime | None] | None,
    channel_cutoff: datetime | None | Literal["unset"],
    expected: tuple[str, str],
) -> None:
    session = DebugSession.create(
        dependency_recipe().render()[0],
        output_dir=tmp_path / "debug",
        channels=[cutoff_channel],
        exclude_newer=global_cutoff,
        exclude_newer_package=package_cutoffs,
        exclude_newer_channel=None if channel_cutoff == "unset" else {cutoff_channel: channel_cutoff},
    )
    assert (session.build_prefix / "share/cutoff-dependency.txt").read_text().strip() == expected[0]
    assert (session.host_prefix / "share/cutoff-other.txt").read_text().strip() == expected[1]
    session.add_packages(["cutoff-dependency"])
    assert (session.host_prefix / "share/cutoff-dependency.txt").read_text().strip() == expected[0]


@pytest.mark.parametrize(
    ("cutoff", "include_unknown", "succeeds"),
    [(CUTOFF, False, False), (CUTOFF, True, True), (None, False, True), (None, True, True)],
)
def test_unknown_timestamps(
    cutoff_channel: str,
    tmp_path: Path,
    cutoff: datetime | None,
    include_unknown: bool,
    succeeds: bool,
) -> None:
    recipe = Stage0Recipe.from_yaml(
        """
package:
  name: unknown-timestamp-result
  version: "1"
requirements:
  host:
    - cutoff-unknown
"""
    )

    def create_session() -> DebugSession:
        return DebugSession.create(
            recipe.render()[0],
            output_dir=tmp_path / "debug",
            channels=[cutoff_channel],
            exclude_newer=cutoff,
            exclude_newer_package={},
            exclude_newer_channel={},
            exclude_newer_include_unknown_timestamp=include_unknown,
        )

    if succeeds:
        assert (create_session().host_prefix / "share/cutoff-unknown.txt").is_file()
    else:
        from rattler_build import BuildError

        with pytest.raises(BuildError):
            create_session()


def test_invalid_package_override(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="invalid exclude_newer_package name"):
        dependency_recipe().render()[0].run_build(
            output_dir=tmp_path / "output",
            channels=[],
            exclude_newer_package={"invalid package name": CUTOFF},
        )


@pytest.mark.parametrize("authentication", ["none", "basic", "token"])
def test_authenticated_channel_cutoff(
    cutoff_http_channel: tuple[str, Path], tmp_path: Path, authentication: str
) -> None:
    base_url, requests_path = cutoff_http_channel
    if authentication == "basic":
        channel = base_url.replace("http://", "http://cutoff-user:cutoff-password@")
    elif authentication == "token":
        channel = base_url + "/t/cutoff-test-token"
    else:
        channel = base_url
    session = DebugSession.create(
        dependency_recipe().render()[0],
        output_dir=tmp_path / "debug",
        channels=[channel],
        exclude_newer_channel={channel: CUTOFF},
        tool_config=ToolConfiguration(use_bz2=False, use_zstd=False, use_sharded=False),
    )
    assert (session.build_prefix / "share/cutoff-dependency.txt").read_text().strip() == "1"
    assert (session.host_prefix / "share/cutoff-other.txt").read_text().strip() == "1"
    requests = [json.loads(line) for line in requests_path.read_text().splitlines()]
    assert any(request["path"].endswith("/noarch/repodata.json") for request in requests)
    if authentication == "basic":
        expected_auth = "Basic " + b64encode(b"cutoff-user:cutoff-password").decode()
        assert all(request["authorization"] == expected_auth for request in requests)
    elif authentication == "token":
        assert all(request["path"].startswith("/t/cutoff-test-token/") for request in requests)
    else:
        assert all(request["authorization"] is None for request in requests)


@pytest.mark.parametrize("channel", ["conda-forge", "relative/channel", "mailto:channel@example.com"])
def test_invalid_channel_override(tmp_path: Path, channel: str) -> None:
    with pytest.raises(ValueError, match="exclude_newer_channel"):
        dependency_recipe().render()[0].run_build(
            output_dir=tmp_path / "output",
            channels=[],
            exclude_newer_channel={channel: CUTOFF},
        )
