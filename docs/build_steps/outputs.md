# Post-build package metadata

## Post-build outputs

Each build-step section receives a unique `OUTPUT_FILE` (also exposed as
`RATTLER_BUILD_OUTPUT_FILE`). Write one dotted field, an optional `.append`
operation, whitespace, and a value per line. Valid JSON preserves native
booleans, numbers, null, lists, and objects; other values are plain strings.
Quote a JSON-looking value such as `"true"` when a string is intended.

For example, an action can collect dependency licenses after running its tools:

```yaml
schema_version: 1
requirements:
  build: [go, go-licenses]
steps:
  - run: |
      go-licenses save ./... --save_path "$BUILD_DIR/go-dependencies"
      dollar='$'
      cat > "$OUTPUT_FILE" <<EOF
      about.repository https://github.com/example/project
      about.license_file.include.append ["$dollar{{ BUILD_DIR }}/go-dependencies/**"]
      requirements.run.append ["libgcc >=14", "zlib"]
      requirements.run_exports.strong.append ["project-abi >=1,<2"]
      EOF
```

Outputs are applied in execution order after all build steps finish, before
packaging. Supported requirement collections are `requirements.run`,
`requirements.run_constraints`, and the `noarch`, `strong`, `weak`,
`strong_constraints`, and `weak_constraints` collections under
`requirements.run_exports`. They update package `index.json` and
`run_exports.json`. Requirements are append-only: replacing finalized
dependencies would be ambiguous.
Embedded rebuild recipes retain the pre-output state, so rebuilding applies
each emitted directive once rather than accumulating changes.

Runtime output cannot change `requirements.build` or `requirements.host`.
Declare those on the action document so the compiler includes them before
solving and installing the environments.

Post-build output can also update `about.*` and packaging fields under
`build.dynamic_linking`, `build.prefix_detection`, `build.files`,
`build.always_copy_files`, `build.always_include_files`, and
`build.post_process`. Append targets are materialized when omitted from the
recipe.

!!! warning "Windows multiline steps"
    On Windows, a multiline `run: |` block is emitted as one command-list item.
    Rattler-Build inserts fail-fast guards between list items, not between the
    physical lines inside one multiline scalar, so check `%errorlevel%` yourself
    when a multiline `cmd.exe` block needs per-line failure handling.


To discover build/host dependencies or generate steps, use
[pre-solve metadata](metadata.md), not this post-build protocol.
