"""
Generate llms.txt and llms-full.txt from the documentation sources.

Zensical does not support the mkdocs-llmstxt plugin, so this script provides
the same entry points for LLMs: an index of the documentation (llms.txt) and
the full documentation content in a single file (llms-full.txt). The files are
written into the docs directory so that they end up at the root of the built
site, and are ignored by git.

llms.txt is written as a template and registered in extra_templates, so that
its links receive the version prefix that mike appends to the site URL at
build time. llms-full.txt contains the raw sources, which may contain
template syntax in examples, so it is copied verbatim instead.
"""

import re
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
DOCS_DIR = REPO_ROOT / "docs"

DESCRIPTION = """\
Rattler-Build is a fast conda package builder that creates cross-platform
relocatable packages from a simple recipe format. The recipe format is heavily
inspired by conda-build and boa, and the output is a standard "conda" package
that can be installed using pixi, mamba or conda.

To build a package, write a `recipe.yaml` file describing the package's
sources, build script, dependencies and tests, then run
`rattler-build build --recipe recipe.yaml`. Rattler-Build is implemented in
Rust, has no dependencies on conda-build or Python and works as a standalone
binary. Packages can be published to prefix.dev, anaconda.org, JFrog
Artifactory, S3 buckets, or Quetz servers with `rattler-build upload`.
See other documentation sections for further details on how to use the
software."""

# Section name -> globs of Markdown files relative to docs/
SECTIONS: dict[str, list[str]] = {
    "Getting Started documentation": [
        "index.md",
        "getting_started.md",
        "highlevel.md",
        "understanding_terminal_output.md",
    ],
    "Examples documentation": [
        "tutorials/*.md",
        "converting_from_conda_build.md",
    ],
    "Build options documentation": [
        "build_options.md",
        "selectors.md",
        "build_script.md",
        "variants.md",
        "config.md",
        "compilers.md",
        "experimental_features.md",
        "v3.md",
        "multiple_output_cache.md",
        "sandbox.md",
        "recipe_generation.md",
        "bump_recipe.md",
        "create_patch.md",
        "debugging_builds.md",
        "tips_and_tricks.md",
        "windows_quirks.md",
    ],
    "Testing documentation": [
        "testing.md",
        "rebuild.md",
    ],
    "Publishing documentation": [
        "authentication_and_upload.md",
        "publish.md",
        "sigstore.md",
        "conda_forge.md",
        "automatic_linting.md",
    ],
    "Package documentation": [
        "package_spec.md",
        "special_files.md",
        "internals.md",
        "system_integration.md",
    ],
    "Python Bindings documentation": [
        "py-rattler-build/reference/*.md",
        "py-rattler-build/tutorials/*.md",
    ],
    "Reference documentation": [
        "reference/*.md",
        "rattler_index.md",
    ],
}


def page_title(source: str, path: Path) -> str:
    """Return the title of a page, preferring frontmatter over the first heading."""
    frontmatter = re.match(r"\A---\n(.*?)\n---\n", source, flags=re.DOTALL)
    if frontmatter:
        title = re.search(r"^title:\s*(.+)$", frontmatter.group(1), flags=re.MULTILINE)
        if title:
            return title.group(1).strip().strip("\"'")
    heading = re.search(r"^#+ (.+)$", source, flags=re.MULTILINE)
    if heading:
        return heading.group(1).strip()
    return path.stem.replace("_", " ").replace("-", " ").capitalize()


def page_url(path: Path) -> str:
    """Return the URL a page is rendered to with directory URLs."""
    relative = path.relative_to(DOCS_DIR)
    parts = relative.parent.parts
    if relative.stem not in ("index", "README"):
        parts += (relative.stem,)
    return "/".join(["{{ base }}", *parts]) + "/"


def main() -> None:
    with open(REPO_ROOT / "zensical.toml", "rb") as f:
        project = tomllib.load(f)["project"]

    header = f"# {project['site_name']}\n\n"
    header += f"> {project['site_description']}\n\n"
    header += f"{DESCRIPTION}\n\n"
    index = '{%- set base = config.site_url | default("") | trim("/") -%}\n' + header
    full = header

    for section, globs in SECTIONS.items():
        pages: list[Path] = []
        for glob in globs:
            pages.extend(sorted(DOCS_DIR.glob(glob)))

        index += f"## {section}\n\n"
        full += f"# {section}\n\n"
        for page in pages:
            source = page.read_text(encoding="utf-8")
            index += f"- [{page_title(source, page)}]({page_url(page)})\n"
            full += f"{source}\n\n"
        index += "\n"

    (DOCS_DIR / "llms.txt").write_text(index, encoding="utf-8")
    (DOCS_DIR / "llms-full.txt").write_text(full, encoding="utf-8")
    print("Generated docs/llms.txt and docs/llms-full.txt")


if __name__ == "__main__":
    main()
