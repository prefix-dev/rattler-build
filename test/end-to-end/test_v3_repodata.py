"""End-to-end coverage for V3 repodata package and channel shapes."""

import json
import os
import struct
from io import BytesIO
from pathlib import Path
from subprocess import CalledProcessError, STDOUT
from typing import Any

import pytest
import zstandard
from helpers import RattlerBuild, get_extracted_package, get_package


V3_RECIPES = Path(__file__).parent.parent.parent / "test-data" / "v3-recipes"


def _package_record_name(package: Path) -> str:
    if package.name.endswith(".tar.bz2"):
        return package.name[: -len(".tar.bz2")]
    if package.name.endswith(".conda"):
        return package.name[: -len(".conda")]
    raise AssertionError(f"unexpected package extension: {package}")


def _v3_submap_for_package(package: Path) -> str:
    if package.name.endswith(".tar.bz2"):
        return "tar.bz2"
    if package.name.endswith(".conda"):
        return "conda"
    raise AssertionError(f"unexpected package extension: {package}")


def _msgpack_load_zst(path: Path) -> Any:
    compressed = BytesIO(path.read_bytes())
    with zstandard.ZstdDecompressor().stream_reader(compressed) as reader:
        data = reader.read()
    value, offset = _read_msgpack(data, 0)
    assert offset == len(data)
    return value


def _read_msgpack(data: bytes, offset: int) -> tuple[Any, int]:
    marker = data[offset]
    offset += 1

    if marker <= 0x7F:
        return marker, offset
    if marker >= 0xE0:
        return marker - 0x100, offset
    if 0x80 <= marker <= 0x8F:
        return _read_map(data, offset, marker & 0x0F)
    if 0x90 <= marker <= 0x9F:
        return _read_array(data, offset, marker & 0x0F)
    if 0xA0 <= marker <= 0xBF:
        return _read_str(data, offset, marker & 0x1F)

    if marker == 0xC0:
        return None, offset
    if marker == 0xC2:
        return False, offset
    if marker == 0xC3:
        return True, offset
    if marker == 0xC4:
        length = data[offset]
        offset += 1
        return data[offset : offset + length], offset + length
    if marker == 0xC5:
        length = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        return data[offset : offset + length], offset + length
    if marker == 0xC6:
        length = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        return data[offset : offset + length], offset + length
    if marker == 0xCC:
        return data[offset], offset + 1
    if marker == 0xCD:
        return struct.unpack_from(">H", data, offset)[0], offset + 2
    if marker == 0xCE:
        return struct.unpack_from(">I", data, offset)[0], offset + 4
    if marker == 0xCF:
        return struct.unpack_from(">Q", data, offset)[0], offset + 8
    if marker == 0xD0:
        return struct.unpack_from(">b", data, offset)[0], offset + 1
    if marker == 0xD1:
        return struct.unpack_from(">h", data, offset)[0], offset + 2
    if marker == 0xD2:
        return struct.unpack_from(">i", data, offset)[0], offset + 4
    if marker == 0xD3:
        return struct.unpack_from(">q", data, offset)[0], offset + 8
    if marker == 0xD9:
        length = data[offset]
        offset += 1
        return _read_str(data, offset, length)
    if marker == 0xDA:
        length = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        return _read_str(data, offset, length)
    if marker == 0xDB:
        length = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        return _read_str(data, offset, length)
    if marker == 0xDC:
        length = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        return _read_array(data, offset, length)
    if marker == 0xDD:
        length = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        return _read_array(data, offset, length)
    if marker == 0xDE:
        length = struct.unpack_from(">H", data, offset)[0]
        offset += 2
        return _read_map(data, offset, length)
    if marker == 0xDF:
        length = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        return _read_map(data, offset, length)

    raise AssertionError(f"unsupported msgpack marker 0x{marker:02x}")


def _read_str(data: bytes, offset: int, length: int) -> tuple[str, int]:
    return data[offset : offset + length].decode(), offset + length


def _read_array(data: bytes, offset: int, length: int) -> tuple[list[Any], int]:
    result = []
    for _ in range(length):
        value, offset = _read_msgpack(data, offset)
        result.append(value)
    return result, offset


def _read_map(data: bytes, offset: int, length: int) -> tuple[dict[Any, Any], int]:
    result = {}
    for _ in range(length):
        key, offset = _read_msgpack(data, offset)
        value, offset = _read_msgpack(data, offset)
        result[key] = value
    return result, offset


def assert_v3_index_json(index_json: dict[str, Any]) -> None:
    assert index_json["repodata_revision"] == 3
    assert index_json["flags"] == ["cuda", "blas:openblas"]
    assert index_json["extra_depends"] == {
        "full": ["pandas >=2", "rich[extras=[jupyter]]"],
        "plot": ["matplotlib >=3.8"],
    }
    assert "scipy[when=\"python >=3.10\"]" in index_json["depends"]
    assert "blas-provider[flags=[blas:*]]" in index_json["depends"]


def assert_legacy_index_json(index_json: dict[str, Any]) -> None:
    assert "repodata_revision" not in index_json
    assert "flags" not in index_json
    assert "extra_depends" not in index_json


def test_v3_build_writes_v3_index_json(
    rattler_build: RattlerBuild, tmp_path: Path
):
    output_dir = tmp_path / "output"

    rattler_build.build(
        V3_RECIPES / "v3-index-shape",
        output_dir,
        extra_args=["--v3", "--test=skip"],
    )

    extracted = get_extracted_package(output_dir, "v3-index-shape")
    index_json = json.loads((extracted / "info/index.json").read_text())
    assert_v3_index_json(index_json)


def test_v3_local_publish_writes_v3_repodata_and_shards(
    rattler_build: RattlerBuild, tmp_path: Path
):
    output_dir = tmp_path / "output"
    channel_dir = tmp_path / "channel"

    rattler_build(
        "publish",
        str(V3_RECIPES / "v3-index-shape"),
        "--to",
        str(channel_dir),
        "--output-dir",
        str(output_dir),
        "--package-format",
        "tar.bz2",
        "--v3",
        "--test=skip",
    )

    package = get_package(channel_dir, "v3-index-shape")
    package_record_name = _package_record_name(package)
    v3_submap = _v3_submap_for_package(package)

    extracted = get_extracted_package(channel_dir, "v3-index-shape")
    index_json = json.loads((extracted / "info/index.json").read_text())
    assert_v3_index_json(index_json)

    subdir = index_json["subdir"]
    repodata = json.loads((channel_dir / subdir / "repodata.json").read_text())
    assert package.name not in repodata["packages"]
    assert package.name not in repodata["packages.conda"]

    record = repodata["v3"][v3_submap][package_record_name]
    assert record["flags"] == ["cuda", "blas:openblas"]
    assert record["extra_depends"] == index_json["extra_depends"]
    assert "scipy[when=\"python >=3.10\"]" in record["depends"]

    shard_index_path = channel_dir / subdir / "repodata_shards.msgpack.zst"
    assert shard_index_path.exists()
    shard_index = _msgpack_load_zst(shard_index_path)
    assert shard_index["info"]["subdir"] == subdir
    assert shard_index["info"]["shards_base_url"] == "./shards/"
    assert "v3-index-shape" in shard_index["shards"]

    digest = shard_index["shards"]["v3-index-shape"].hex()
    shard = _msgpack_load_zst(
        channel_dir / subdir / "shards" / f"{digest}.msgpack.zst"
    )
    shard_record = shard["v3"][v3_submap][package_record_name]
    assert shard_record["flags"] == record["flags"]
    assert shard_record["extra_depends"] == record["extra_depends"]
    assert shard_record["depends"] == record["depends"]


def test_legacy_build_keeps_legacy_index_json(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    output_dir = tmp_path / "output"

    rattler_build.build(
        recipes / "legacy-index-shape", output_dir, extra_args=["--test=skip"]
    )

    extracted = get_extracted_package(output_dir, "legacy-index-shape")
    index_json = json.loads((extracted / "info/index.json").read_text())
    assert_legacy_index_json(index_json)


@pytest.mark.parametrize(
    ("recipe_name", "expected", "hint"),
    [
        (
            "v3-build-flags-rejected",
            "package flags require the --v3 flag",
            "Enable --v3 to use build.flags.",
        ),
        (
            "v3-extras-rejected",
            "requirements.extras",
            "Enable --v3 to use requirements.extras.",
        ),
        (
            "v3-conditional-matchspec-rejected",
            "invalid bracket key: when",
            "Enable --v3 to use V3 MatchSpec keys",
        ),
        (
            "v3-flags-matchspec-rejected",
            "invalid bracket key: flags",
            "Enable --v3 to use V3 MatchSpec keys",
        ),
    ],
)
def test_v3_fields_are_rejected_without_opt_in(
    rattler_build: RattlerBuild,
    tmp_path: Path,
    recipe_name: str,
    expected: str,
    hint: str,
):
    args = rattler_build.build_args(
        V3_RECIPES / recipe_name,
        tmp_path / "output",
        extra_args=["--test=skip"],
    )
    with pytest.raises(CalledProcessError) as exc_info:
        rattler_build(
            *args,
            stderr=STDOUT,
            env={**os.environ, "NO_COLOR": "1"},
        )

    assert expected in str(exc_info.value.output)
    assert hint in str(exc_info.value.output)
