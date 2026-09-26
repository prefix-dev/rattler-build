# Caching and staging

## Caching build steps

Each build step receives `RATTLER_BUILD_STEP_CACHE`, pointing to a persistent
declaration file under the build directory. A successful step can write cache
conditions to this file:

```yaml
- name: compile
  run: |
    cmake --build "$SRC_DIR/build"
    cat > "$RATTLER_BUILD_STEP_CACHE" <<'EOF'
    input-hash: CMakeLists.txt
    input-hash: src/**
    output-mtime: build/**
    EOF
```

On Windows, write the same lines to `%RATTLER_BUILD_STEP_CACHE%`. Each line is
`KEY: GLOB`; blank lines and `#` comments are ignored. `input-hash` and
`output-hash` compare matching paths and contents. `input-mtime` and
`output-mtime` compare paths, sizes, and modification times.
Hash conditions include symlink identity and follow directory links using paths
relative to the step working directory. Modification-time conditions inspect
links themselves rather than their targets.

Globs use `/`, are relative to the step working directory, and cannot be
absolute or contain `..`. Every condition must match an entry. Missing inputs,
deleted outputs, or changes to the script, interpreter, effective environment,
working directory, or compiled plan invalidate the cache.

After success, rattler-build stores fingerprints and a checksum-verified copy
of `OUTPUT_FILE` outside disposable `work/`. A hit restores this metadata,
including intentional absence. Missing or altered replay data causes a miss.
Before executing a miss, the previous success record and declaration are
removed: a failed rerun cannot revive stale success. A successful step must
write its declaration again to remain cacheable. The adjacent `.state.json`
and `.output` files belong to the executor and should not be edited.

See [`examples/step-cache`](https://github.com/prefix-dev/rattler-build/tree/main/examples/step-cache)
for a cross-platform example, and
[`examples/adjacent`](https://github.com/prefix-dev/rattler-build/tree/main/examples/adjacent)
for a CMake pipeline.


## Staging steps

Staging actions use the same step execution and cache transactions as package
actions. `RATTLER_BUILD_STEP_CACHE` is available, and successful declarations
are recorded; a whole-stage cache hit still bypasses execution of the stage.

Staging outputs do not have a package identity. Writing package metadata to
`OUTPUT_FILE` from a staging step is rejected with an error before a success
record is committed. Put metadata-producing actions, such as dependency-license
collection, on the inheriting package output instead.


A step cache does not restore build artifacts: it verifies that declared outputs
still exist and replays only the step’s metadata. Declare outputs as well as
inputs; files outside the declared working-directory globs are not verified.

Dangling symlinks that do not match a declaration are ignored. A declared dangling
link, a directory cycle, or unreadable inputs cannot establish a valid fingerprint.
