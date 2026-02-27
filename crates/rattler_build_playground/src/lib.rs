use std::cell::RefCell;
use std::str::FromStr;

use arborium::Highlighter;
use arborium::theme::builtin;
use indexmap::IndexMap;
use rattler_build_jinja::{JinjaConfig, Variable};
use rattler_build_recipe::{
    stage0::{self, Recipe},
    stage1::{Evaluate, EvaluationContext},
    variant_render::{RenderConfig, render_recipe_with_variant_config},
};
use rattler_build_variant_config::{VariantConfig, parse_conda_build_config};
use rattler_conda_types::Platform;
use serde::Serialize;
use wasm_bindgen::prelude::*;

thread_local! {
    static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::new());
}

/// Serialize a value to YAML and syntax-highlight it, returning HTML.
fn highlight_yaml(value: &impl Serialize) -> Result<String, String> {
    let yaml = serde_yaml::to_string(value).map_err(|e| e.to_string())?;
    HIGHLIGHTER.with(|hl| {
        hl.borrow_mut()
            .highlight("yaml", &yaml)
            .map_err(|e| e.to_string())
    })
}

/// Build a JSON success response containing highlighted HTML.
fn ok_html(html: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "ok": true,
        "result_html": html,
    }))
    .expect("serialization of ok response cannot fail")
}

/// Initialize the panic hook for better WASM error messages.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Return the CSS for the syntax-highlighting theme.
///
/// Call once after WASM init and inject into a `<style>` element.
#[wasm_bindgen]
pub fn get_theme_css() -> String {
    let theme = builtin::catppuccin_mocha();
    let output_css = theme.to_css("pre.output-yaml");
    let editor_css = theme.to_css("pre.editor-highlight");
    format!("{output_css}\n{editor_css}")
}

/// Syntax-highlight a raw YAML source string, returning HTML.
///
/// Used by the editor overlay to highlight user input in real time.
#[wasm_bindgen]
pub fn highlight_source_yaml(source: &str) -> String {
    HIGHLIGHTER
        .with(|hl| {
            hl.borrow_mut()
                .highlight("yaml", source)
                .map_err(|e| e.to_string())
        })
        .unwrap_or_else(|_| {
            // Fallback: return HTML-escaped source
            source
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        })
}

/// Parse a recipe YAML string to Stage 0 (preserving templates and conditionals).
///
/// Returns a JSON string: `{ "ok": true, "result_html": "..." }` or `{ "ok": false, "error": {...} }`
#[wasm_bindgen]
pub fn parse_recipe(yaml_source: &str) -> String {
    match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(recipe) => match highlight_yaml(&recipe) {
            Ok(html) => ok_html(&html),
            Err(e) => error_json(&e, None, None),
        },
        Err(e) => format_parse_error(&e),
    }
}

/// Evaluate a recipe with variables and a target platform.
///
/// - `yaml_source`: The recipe YAML string
/// - `variables_json`: JSON object mapping variable names to values, e.g. `{"python": "3.11"}`
/// - `target_platform`: Platform string like "linux-64", "osx-arm64", etc.
///
/// Returns a JSON string: `{ "ok": true, "result_html": "..." }` or `{ "ok": false, "error": {...} }`
#[wasm_bindgen]
pub fn evaluate_recipe(yaml_source: &str, variables_json: &str, target_platform: &str) -> String {
    // Parse the recipe to Stage 0
    let recipe = match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(r) => r,
        Err(e) => return format_parse_error(&e),
    };

    // Parse variables from JSON
    let variables = match parse_variables(variables_json) {
        Ok(v) => v,
        Err(e) => return error_json(&format!("Invalid variables JSON: {e}"), None, None),
    };

    // Parse platform
    let platform = Platform::from_str(target_platform).unwrap_or(Platform::Linux64);

    let jinja_config = JinjaConfig {
        target_platform: platform,
        build_platform: platform,
        host_platform: platform,
        experimental: false,
        recipe_path: None,
        ..Default::default()
    };

    let context = EvaluationContext::with_variables_and_config(variables, jinja_config);

    match &recipe {
        Recipe::SingleOutput(r) => {
            // Evaluate context section if present
            let eval_context = if !r.context.is_empty() {
                match context.with_context(&r.context) {
                    Ok((ctx, _)) => ctx,
                    Err(e) => return format_parse_error(&e),
                }
            } else {
                context
            };

            match r.evaluate(&eval_context) {
                Ok(stage1) => match highlight_yaml(&stage1) {
                    Ok(html) => ok_html(&html),
                    Err(e) => error_json(&e, None, None),
                },
                Err(e) => format_parse_error(&e),
            }
        }
        Recipe::MultiOutput(r) => {
            let eval_context = if !r.context.is_empty() {
                match context.with_context(&r.context) {
                    Ok((ctx, _)) => ctx,
                    Err(e) => return format_parse_error(&e),
                }
            } else {
                context
            };

            match r.evaluate(&eval_context) {
                Ok(outputs) => match highlight_yaml(&outputs) {
                    Ok(html) => ok_html(&html),
                    Err(e) => error_json(&e, None, None),
                },
                Err(e) => format_parse_error(&e),
            }
        }
    }
}

/// Get the list of variables used in a recipe (for UI hints).
///
/// Returns a JSON string with both structured data and highlighted HTML:
/// `{ "ok": true, "result": [...], "result_html": "..." }` or `{ "ok": false, "error": {...} }`
#[wasm_bindgen]
pub fn get_used_variables(yaml_source: &str) -> String {
    match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(recipe) => {
            let vars = match &recipe {
                Recipe::SingleOutput(r) => r.used_variables(),
                Recipe::MultiOutput(r) => r.used_variables(),
            };
            // Return both highlighted YAML and structured JSON
            // (the JSON array is still needed for the used-vars hint in the UI)
            let html = highlight_yaml(&vars).unwrap_or_default();
            serde_json::to_string(&serde_json::json!({
                "ok": true,
                "result": vars,
                "result_html": html,
            }))
            .expect("serialization of ok response cannot fail")
        }
        Err(e) => format_parse_error(&e),
    }
}

/// Get available platform strings for the UI dropdown.
#[wasm_bindgen]
pub fn get_platforms() -> String {
    let platforms: Vec<&str> = Platform::all().map(|p| p.as_str()).collect();
    serde_json::to_string(&platforms).unwrap_or_default()
}

/// A concise summary of a rendered variant for display in the UI
#[derive(Serialize)]
struct VariantSummary {
    /// Package name
    name: String,
    /// Package version
    version: String,
    /// Build string (resolved)
    build_string: Option<String>,
    /// Whether this output is skipped
    skipped: bool,
    /// Whether this is a noarch package
    noarch: Option<String>,
    /// Variant keys and values
    variant: Vec<(String, String)>,
    /// Build dependencies (just names)
    build_deps: Vec<String>,
    /// Host dependencies (just names)
    host_deps: Vec<String>,
    /// Run dependencies (display strings)
    run_deps: Vec<String>,
    /// Resolved context variables (key -> evaluated JSON value)
    context: IndexMap<String, Variable>,
}

/// Render a recipe with variant configuration, producing all output variants.
///
/// - `yaml_source`: The recipe YAML string
/// - `variant_config_yaml`: Variant configuration YAML (e.g. `python:\n  - "3.11"\n  - "3.12"`)
/// - `target_platform`: Platform string like "linux-64", "osx-arm64", etc.
///
/// Returns a JSON string with summary cards and highlighted YAML:
/// `{ "ok": true, "result": { "variants_html": "...", "summary": [...concise...] } }`
#[wasm_bindgen]
pub fn render_variants(
    yaml_source: &str,
    variant_config_yaml: &str,
    target_platform: &str,
    variant_format: &str,
) -> String {
    // Parse the recipe to Stage 0
    let stage0_recipe = match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(r) => r,
        Err(e) => return format_parse_error(&e),
    };

    // Parse platform
    let platform = Platform::from_str(target_platform).unwrap_or(Platform::Linux64);

    let jinja_config = JinjaConfig {
        target_platform: platform,
        build_platform: platform,
        host_platform: platform,
        experimental: false,
        recipe_path: None,
        ..Default::default()
    };

    let variant_config = if variant_format == "conda_build_config" {
        match parse_conda_build_config(variant_config_yaml, &jinja_config) {
            Ok(vc) => vc,
            Err(e) => return error_json(&format!("Invalid conda_build_config: {e}"), None, None),
        }
    } else {
        match VariantConfig::from_yaml_str(variant_config_yaml) {
            Ok(vc) => vc,
            Err(e) => return error_json(&format!("Invalid variant config: {e}"), None, None),
        }
    };

    let render_config = RenderConfig::new()
        .with_target_platform(platform)
        .with_host_platform(platform)
        .with_build_platform(platform);

    // Render with variant config
    match render_recipe_with_variant_config(&stage0_recipe, &variant_config, render_config) {
        Ok(rendered) => {
            // Build concise summaries
            let summary: Vec<VariantSummary> = rendered
                .iter()
                .map(|rv| {
                    let recipe = &rv.recipe;
                    let build_string = recipe.build.string.as_resolved().map(|s| s.to_string());
                    let noarch = recipe.build.noarch.and_then(|n| {
                        if n.is_none() {
                            None
                        } else if n.is_python() {
                            Some("python".to_string())
                        } else {
                            Some("generic".to_string())
                        }
                    });

                    let variant: Vec<(String, String)> = rv
                        .variant
                        .iter()
                        .map(|(k, v)| (k.0.clone(), v.to_string()))
                        .collect();

                    let build_deps: Vec<String> = recipe
                        .requirements
                        .build
                        .iter()
                        .filter_map(|d| d.name().map(|n| n.as_normalized().to_string()))
                        .collect();

                    let host_deps: Vec<String> = recipe
                        .requirements
                        .host
                        .iter()
                        .filter_map(|d| d.name().map(|n| n.as_normalized().to_string()))
                        .collect();

                    let run_deps: Vec<String> = recipe
                        .requirements
                        .run
                        .iter()
                        .map(|d| d.to_string())
                        .collect();

                    VariantSummary {
                        name: recipe.package.name.as_normalized().to_string(),
                        version: recipe.package.version.to_string(),
                        build_string,
                        skipped: recipe.build.skip,
                        noarch,
                        variant,
                        build_deps,
                        host_deps,
                        run_deps,
                        context: recipe.context.clone(),
                    }
                })
                .collect();

            // Highlight full variant data as YAML
            let variants_html = highlight_yaml(&rendered).unwrap_or_default();

            let result = serde_json::json!({
                "ok": true,
                "result": {
                    "variants_html": variants_html,
                    "summary": summary,
                },
            });
            serde_json::to_string(&result)
                .expect("serialization of ok response cannot fail")
        }
        Err(e) => error_json(&e.to_string(), None, None),
    }
}

fn parse_variables(json: &str) -> Result<IndexMap<String, Variable>, String> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut result = IndexMap::new();
    for (key, value) in map {
        let var = match value {
            serde_json::Value::Bool(b) => Variable::from(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Variable::from(i)
                } else {
                    Variable::from(n.to_string())
                }
            }
            serde_json::Value::String(s) => Variable::from(s),
            other => Variable::from(other.to_string()),
        };
        result.insert(key, var);
    }
    Ok(result)
}

/// Parse variant config YAML and return a JSON object with the first value of each key.
///
/// Used by the "Evaluated" tab to build a simple variables map from the variant config.
/// For example, `python:\n  - "3.11"\n  - "3.12"` becomes `{"python": "3.11"}`.
#[wasm_bindgen]
pub fn first_variant_values(variant_yaml: &str) -> String {
    let parsed: Result<IndexMap<String, serde_yaml::Value>, _> =
        serde_yaml::from_str(variant_yaml);

    let map = match parsed {
        Ok(m) => m,
        Err(_) => return "{}".to_string(),
    };

    let mut result = serde_json::Map::new();
    for (key, value) in map {
        let first = match &value {
            serde_yaml::Value::Sequence(seq) => seq.first().unwrap_or(&value).clone(),
            other => other.clone(),
        };
        let json_val = match first {
            serde_yaml::Value::Bool(b) => serde_json::Value::Bool(b),
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    serde_json::Value::Number(i.into())
                } else if let Some(f) = n.as_f64() {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::String(n.to_string()))
                } else {
                    serde_json::Value::String(n.to_string())
                }
            }
            serde_yaml::Value::String(s) => serde_json::Value::String(s),
            other => serde_json::Value::String(format!("{other:?}")),
        };
        result.insert(key, json_val);
    }

    serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
}

fn error_json(message: &str, line: Option<usize>, column: Option<usize>) -> String {
    let escaped = serde_json::to_string(message).unwrap_or_else(|_| format!(r#""{message}""#));
    match (line, column) {
        (Some(l), Some(c)) => {
            format!(r#"{{"ok":false,"error":{{"message":{escaped},"line":{l},"column":{c}}}}}"#)
        }
        _ => format!(r#"{{"ok":false,"error":{{"message":{escaped}}}}}"#),
    }
}

fn format_parse_error(e: &rattler_build_yaml_parser::ParseError) -> String {
    let message = e.to_string();
    match e {
        rattler_build_yaml_parser::ParseError::IoError { .. } => error_json(&message, None, None),
        _ => {
            let span = e.span();
            if let Some(start) = span.start() {
                error_json(&message, Some(start.line()), Some(start.column()))
            } else {
                error_json(&message, None, None)
            }
        }
    }
}
