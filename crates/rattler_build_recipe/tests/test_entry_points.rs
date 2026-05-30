use miette::Diagnostic;
use rattler_build_recipe::stage0::parse_recipe_from_source;

const RECIPE_HEADER: &str = "
package:
  name: test
  version: 1.0.0
";

#[derive(Debug)]
struct ErrorDetails {
    message: String,
    help: Option<String>,
}

fn parse_with_entry_points(entry_points_yaml: &str) -> Result<(), ErrorDetails> {
    let yaml = format!(
        "{}\nbuild:\n  python:\n    entry_points:\n{}\n",
        RECIPE_HEADER, entry_points_yaml,
    );
    parse_recipe_from_source(&yaml).map(|_| ()).map_err(|e| ErrorDetails {
        message: format!("{}", e),
        help: e.help().map(|h| format!("{}", h)),
    })
}

/// Regression test for https://github.com/prefix-dev/rattler-build/issues/2523.
///
/// The buggy gguf-feedstock recipe used `command = "module:function"` with
/// literal quotes wrapping the value. YAML preserves those quotes (this is a
/// plain scalar, not a quoted one), so the module ends up being `"scripts`
/// and is not a valid Python dotted identifier. We should reject this early
/// with a clear error rather than write invalid entry points into link.json.
#[test]
fn malformed_quoted_entry_point_is_rejected_at_parse_time() {
    let err = parse_with_entry_points(
        r#"      - gguf-convert-endian = "scripts:gguf_convert_endian_entrypoint""#,
    )
    .expect_err("expected parsing to fail for malformed entry point");

    assert!(
        err.message.contains("python.entry_points"),
        "error should name the field, got: {}",
        err.message,
    );
    assert!(
        err.message.contains("Invalid entry point"),
        "error should mention the invalid entry point, got: {}",
        err.message,
    );
    assert!(
        err.message.contains("Python dotted identifier"),
        "error should explain the underlying validation, got: {}",
        err.message,
    );
    let help = err.help.expect("error should carry a help suggestion");
    assert!(
        help.contains("command = module:function"),
        "help should suggest the correct format, got: {help}",
    );
}

#[test]
fn valid_entry_point_parses() {
    parse_with_entry_points(r"      - flask = flask.cli:main").expect("valid entry point");
}

#[test]
fn entry_point_missing_equals_is_rejected() {
    let err = parse_with_entry_points(r"      - flask flask.cli:main")
        .expect_err("missing '=' separator");
    assert!(
        err.message.contains("python.entry_points"),
        "got: {}",
        err.message,
    );
}

#[test]
fn entry_point_missing_colon_is_rejected() {
    let err = parse_with_entry_points(r"      - flask = flask.cli.main")
        .expect_err("missing ':' separator");
    assert!(
        err.message.contains("python.entry_points"),
        "got: {}",
        err.message,
    );
}
