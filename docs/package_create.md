# Creating packages from prebuilt files

`rattler-build package create` turns files that another build system already
produced (Bazel, Buck, Nix, CMake install trees, ...) into a conda package,
without a recipe or build script. You describe the files and the package
metadata in a JSON manifest. rattler-build stages the files, relinks binaries
so they find their libraries inside the environment, writes the `info/`
metadata and creates the archive.

```bash
rattler-build package create --manifest package.json --output-dir dist
# dist/greet-1.0.0-h1234_0.conda
```

## The manifest

```json
{
  "name": "greet",
  "version": "1.0.0",
  "build_string": "h1234_0",
  "target_platform": "linux-64",
  "depends": ["libzlib >=1.3.2,<2.0a0"],
  "license": "MIT",
  "summary": "Greets people",
  "timestamp": 1672531200000,
  "files": [
    {"src": "build/bin/greet", "dest": "bin/greet", "executable": true},
    {"src": "build/lib/libgreet.so", "dest": "lib/libgreet.so"},
    {"src": "src/greet.h", "dest": "include/greet.h"}
  ],
  "symlinks": [
    {"dest": "bin/hi", "target": "greet"}
  ]
}
```

| Field | Description |
|---|---|
| `name`, `version`, `build_string` | Package identity; the output file is `<name>-<version>-<build_string>.conda`. |
| `build_number` | Build number (default `0`). |
| `target_platform` | Subdir, e.g. `linux-64` or `osx-arm64`; `noarch` together with `noarch`. |
| `noarch` | `generic` or `python`, for `noarch` packages. |
| `depends`, `constrains` | Run requirements and run constraints (MatchSpecs). |
| `run_exports` | The package's own run exports, e.g. `{"weak": ["greet >=1.0"]}`. |
| `track_features` | Track features. |
| `license`, `license_family`, `summary`, `description`, `homepage`, `repository`, `documentation` | Package metadata (`index.json` / `about.json`). |
| `license_files` | Files copied to `info/licenses/`. |
| `files` | `{"src", "dest", "executable"}`: `src` is a path on disk, `dest` the path inside the package. |
| `symlinks` | `{"dest", "target"}`: symlinks created inside the package. |
| `timestamp` | Milliseconds since the epoch, used for the archive and `index.json`. |
| `relink` | Relink ELF / Mach-O files (default `true`). |
| `rpaths` | Rpaths (relative to the prefix) set while relinking (default `["lib/"]`). |
| `strip_rpath_patterns` | Rpath entries containing any of these substrings are replaced before relinking (see below). |
| `detect_prefix` | Detect and record prefix placeholders (default `false`). |
| `compression_level`, `compression_threads` | zstd settings (defaults `15` and `1`). |

Unknown fields are rejected. Destinations must stay inside the package; the
`info/` directory is reserved.

## Relinking

Binaries are relinked like in a regular `rattler-build build`: rpaths that
point outside the package are removed, and `$ORIGIN`-relative paths to the
`rpaths` directories are added (`$ORIGIN/../lib` for `bin/greet`, `$ORIGIN/`
for `lib/libgreet.so`).

Build systems often link through their own trees with `$ORIGIN`-relative
rpaths (Bazel's `_solib_*` directories and runfiles, for example). Those
can look like they point *inside* the package, so they would be kept. List
them in `strip_rpath_patterns` to have them replaced in place:

```json
"strip_rpath_patterns": ["_solib_", ".runfiles/"]
```

## Reproducibility

With a fixed `timestamp` (and the default single compression thread) the
same manifest and inputs produce byte-for-byte identical archives. Prefix
detection is off by default, because files built outside a conda build
prefix don't contain one.
