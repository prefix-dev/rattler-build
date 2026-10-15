import os
from pathlib import Path

import pytest
from helpers import RattlerBuild

# `pb-a` and `pb-b` are independent; each waits for the other to have started,
# so the build only succeeds if they run at the same time. `pb-c` needs both.
RECIPE = """\
schema_version: 1

recipe:
  name: parallel-builds
  version: "1.0.0"

outputs:
  - package:
      name: pb-a
    build:
      script:
        - touch "{markers}/a.started"
        - for i in $(seq 1 300); do [ -e "{markers}/b.started" ] && break; sleep 0.1; done
        - test -e "{markers}/b.started"
        - mkdir -p $PREFIX/share && touch $PREFIX/share/pb-a

  - package:
      name: pb-b
    build:
      script:
        - touch "{markers}/b.started"
        - for i in $(seq 1 300); do [ -e "{markers}/a.started" ] && break; sleep 0.1; done
        - test -e "{markers}/a.started"
        - mkdir -p $PREFIX/share && touch $PREFIX/share/pb-b

  - package:
      name: pb-c
    requirements:
      host:
        - pb-a
        - pb-b
    build:
      script:
        - test -e $PREFIX/share/pb-a
        - test -e $PREFIX/share/pb-b
        - printf '%s' "${{MAKEFLAGS:-}}" > "{markers}/c.makeflags"
        - mkdir -p $PREFIX/share && touch $PREFIX/share/pb-c
"""


@pytest.mark.skipif(os.name == "nt", reason="the recipe uses bash scripts")
@pytest.mark.parametrize("parallel_builds", ["2", "2:4"])
def test_parallel_builds(
    rattler_build: RattlerBuild, tmp_path: Path, parallel_builds: str
):
    """Independent outputs build side by side, and a dependent output after
    them; only with `:JOBS` do build scripts see the jobserver."""
    markers = tmp_path / "markers"
    markers.mkdir()
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()
    (recipe_dir / "recipe.yaml").write_text(RECIPE.format(markers=markers))
    output = tmp_path / "output"

    rattler_build.build(
        recipe_dir,
        output,
        extra_args=["--parallel-builds", parallel_builds, "--test=skip"],
    )

    built = {p.name.split("-1.0.0")[0] for p in output.glob("*/pb-*.tar.bz2")}
    assert built == {"pb-a", "pb-b", "pb-c"}

    makeflags = (markers / "c.makeflags").read_text()
    if ":" in parallel_builds:
        assert makeflags.startswith("-j4 --jobserver-auth=fifo:")
    else:
        assert makeflags == ""
