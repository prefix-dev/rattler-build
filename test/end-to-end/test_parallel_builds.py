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


# Rendezvous requires both variants to run concurrently; the shared marker
# directory rejects concurrent builds when rendezvous is disabled.
VARIANTS_RECIPE = """\
schema_version: 1

package:
  name: pb-variants
  version: "1.0.0"

build:
  script:
    - test ! -e "$SRC_DIR/claimed"
    - touch "$SRC_DIR/claimed"
    - printf '%s' "$SRC_DIR" > "{markers}/${{{{ pbv }}}}.work_dir"
    - touch "{markers}/${{{{ pbv }}}}.started"
    - if [ "{rendezvous}" = "1" ]; then for i in $(seq 1 300); do [ "$(ls {markers}/*.started | wc -l)" -ge 2 ] && break; sleep 0.1; done; fi
    - if [ "{rendezvous}" = "1" ]; then [ "$(ls {markers}/*.started | wc -l)" -ge 2 ]; fi
    - if [ "{rendezvous}" = "0" ]; then mkdir "{markers}/running" && sleep 2 && rmdir "{markers}/running"; fi
    - test -e "$SRC_DIR/claimed"
    - mkdir -p $PREFIX/share && touch $PREFIX/share/pb-variants-${{{{ pbv }}}}
"""


def _build_two_variants(
    rattler_build: RattlerBuild,
    tmp_path: Path,
    rendezvous: bool,
    extra_args: list[str],
) -> dict[str, str]:
    markers = tmp_path / "markers"
    markers.mkdir()
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()
    (recipe_dir / "recipe.yaml").write_text(
        VARIANTS_RECIPE.format(markers=markers, rendezvous="1" if rendezvous else "0")
    )
    (recipe_dir / "variants.yaml").write_text('pbv:\n  - "1"\n  - "2"\n')
    output = tmp_path / "output"

    rattler_build.build(
        recipe_dir,
        output,
        variant_config=recipe_dir / "variants.yaml",
        extra_args=["--parallel-builds", "2", "--test=skip", *extra_args],
    )

    assert len(list(output.glob("*/pb-variants-1.0.0-*.tar.bz2"))) == 2
    return {v: (markers / f"{v}.work_dir").read_text() for v in ("1", "2")}


@pytest.mark.skipif(os.name == "nt", reason="the recipe uses bash scripts")
def test_parallel_builds_of_variants_use_separate_build_dirs(
    rattler_build: RattlerBuild, tmp_path: Path
):
    work_dirs = _build_two_variants(
        rattler_build, tmp_path, rendezvous=True, extra_args=[]
    )
    assert work_dirs["1"] != work_dirs["2"]


@pytest.mark.skipif(os.name == "nt", reason="the recipe uses bash scripts")
def test_parallel_builds_of_variants_sharing_a_build_dir_take_turns(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Each variant must start with a clean work directory."""
    work_dirs = _build_two_variants(
        rattler_build, tmp_path, rendezvous=False, extra_args=["--no-build-id"]
    )
    assert work_dirs["1"] == work_dirs["2"]
