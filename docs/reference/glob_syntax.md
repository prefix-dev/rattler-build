# Glob syntax

Recipe fields that select paths use the Rust
[`globset`](https://docs.rs/globset/latest/globset/#syntax) syntax.

| Pattern | Matches |
|---------|---------|
| `?` | Any single character |
| `*` | Zero or more characters, including path separators |
| `**` | Directories recursively |
| `{a,b}` | Either `a` or `b`; alternatives cannot be nested |
| `[ab]` | Either `a` or `b` |
| `[a-z]` | Any character in the range `a` through `z` |
| `[!ab]` | Any character except `a` or `b` |
| `[*]` | A literal `*` |

A pattern that names a directory also matches its contents. For example, both
`share/` and `share` match files below that directory. `.` and `./` match
everything.

Alternatives can replace platform selectors when only a file extension differs:

```yaml
build:
  files:
    - lib/libclang-cpp.{dylib,so}
```

This matches `lib/libclang-cpp.dylib` and `lib/libclang-cpp.so`.

Patterns can also use an `include`/`exclude` mapping. If `include` is empty,
every path is included before the exclusions are applied:

```yaml
build:
  files:
    include:
      - include/**/*.h
    exclude:
      - include/**/private.h
```

The matching root depends on the field:

- `source.filter` patterns are relative to the copied or extracted source tree.
- `build.files`, `always_include_files`, and other build file patterns are
  relative to the package prefix.
- `tests.package_contents` patterns are relative to the installed test prefix.

YAML quotes are required when a pattern starts with `*`, because an unquoted
asterisk denotes a YAML alias. For example, write `"**/*.txt"`, not
`**/*.txt`.
