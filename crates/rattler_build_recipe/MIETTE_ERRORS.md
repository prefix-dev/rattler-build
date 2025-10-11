# Beautiful Error Messages with Miette

This crate supports optional [miette](https://docs.rs/miette/) integration for beautiful, user-friendly error diagnostics.

## Enabling Miette Support

Add the `miette` feature to your `Cargo.toml`:

```toml
[dependencies]
rattler_build_recipe = { version = "0.1.0", features = ["miette"] }
```

## Using Source Tracking

When the `miette` feature is enabled, you can use the `Source` type to track source code for excellent error reporting:

```rust
use rattler_build_recipe::source_code::Source;
use rattler_build_recipe::stage0::parse_recipe_from_source;

// Load a recipe from a file
let source = Source::from_path(Path::new("recipe.yaml"))?;

// Parse the recipe - errors will include span information
match parse_recipe_from_source(source.as_ref()) {
    Ok(recipe) => println!("Parsed successfully!"),
    Err(err) => {
        // With miette, you can format beautiful error messages
        eprintln!("{:?}", miette::Report::new(err));
    }
}
```

## Error Types

All `ParseError` types implement `miette::Diagnostic` when the feature is enabled:

```rust
use rattler_build_recipe::{ParseError, ErrorKind, Span};

let error = ParseError::missing_field("name", span)
    .with_suggestion("add a 'name' field to the package section");

// Pretty-print with miette
println!("{:?}", miette::Report::new(error));
```

## Example Error Messages

### Missing Required Field

```yaml
about:
  homepage: https://example.com
  license: MIT
```

**Error:**
```
missing field
  ┌─ recipe.yaml:1:6
  │
1 │ about:
  │      ^ missing required field: package
```

### Unknown Field with Suggestion

```yaml
package:
  name: my-package
  version: 1.0.0

unknown_section:
  value: something
```

**Error:**
```
invalid value
  ┌─ recipe.yaml:5:1
  │
5 │ unknown_section:
  │ ^^^^^^^^^^^^^^^ invalid value for recipe: unknown top-level field 'unknown_section'
  │
  = help: valid top-level fields are: package, about, requirements, extra
```

### Invalid Jinja Template

```yaml
package:
  name: '${{ name | invalid_filter(unclosed }}'
  version: 1.0.0
```

**Error:**
```
Jinja template error
  ┌─ recipe.yaml:2:9
  │
2 │   name: '${{ name | invalid_filter(unclosed }}'
  │         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  │         Jinja error: Failed to parse Jinja template: syntax error: unexpected `}`, expected `,`
```

## Testing Error Messages

The crate includes snapshot tests for error messages. Run them with:

```bash
cargo test --features miette error_tests
```

To review and update error message snapshots:

```bash
cargo insta review
```

## Features

When the `miette` feature is enabled, you get:

- ✅ Precise source location tracking
- ✅ Beautiful terminal output with syntax highlighting
- ✅ Helpful error messages with suggestions
- ✅ Context lines showing where errors occurred
- ✅ Support for named sources (file paths)
- ✅ Diagnostic codes for error categorization

## Without Miette

If you don't enable the `miette` feature, errors still work but with simpler formatting:

```rust
// Still works, just with basic Display formatting
let err = ParseError::missing_field("name", span);
println!("{}", err);
// Outputs: "missing field at 1:1: missing required field: name"
```
