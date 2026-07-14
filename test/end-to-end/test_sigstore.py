import hashlib
import json
import shutil
from pathlib import Path

from helpers import RattlerBuild


FLASK_SHA256 = "284c7b8f2f58cb737f0cf1c30fd7eaf0ccfcde196099d24ecede3fc2005aa59e"


def _recipe(path: Path) -> Path:
    recipe = path / "recipe.yaml"
    recipe.write_text(
        f"""
package:
  name: sigstore-source-test
  version: 1.0
source:
  url: https://pypi.io/packages/source/f/flask/flask-3.1.1.tar.gz
  sha256: {FLASK_SHA256}
  attestation:
    publishers:
      - github:pallets/flask
build:
  number: 0
"""
    )
    return recipe


def _run(rattler_build: RattlerBuild, recipe: Path, output: Path):
    return rattler_build(
        *rattler_build.build_args(recipe, output, extra_args=["--experimental"]),
        capture_output=True,
        text=True,
    )


def _build(rattler_build: RattlerBuild, recipe: Path, output: Path):
    result = _run(rattler_build, recipe, output)
    assert result.returncode == 0, result.stdout + result.stderr
    return result.stdout + result.stderr


def test_pypi_sigstore_verification_and_tampered_cache(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Exercise the real PyPI provenance API and reject a corrupted cached archive."""
    recipe = _recipe(tmp_path)
    output = tmp_path / "output"
    shutil.rmtree(output, ignore_errors=True)

    first_log = _build(rattler_build, recipe, output)
    assert "Attestation verified" in first_log

    metadata_file = next((output / "src_cache" / ".metadata").glob("*.json"))
    metadata = json.loads(metadata_file.read_text())
    archive = output / "src_cache" / metadata["cache_path"]
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == FLASK_SHA256

    archive.write_bytes(b"tampered")
    assert hashlib.sha256(archive.read_bytes()).hexdigest() != FLASK_SHA256

    _build(rattler_build, recipe, output)
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == FLASK_SHA256

    recipe.write_text(recipe.read_text().replace("github:pallets/flask", "github:pallets/werkzeug"))
    rejected = _run(rattler_build, recipe, output)
    assert rejected.returncode != 0
    assert "found identity:" in rejected.stdout + rejected.stderr
    assert "github.com/pallets/flask" in rejected.stdout + rejected.stderr
