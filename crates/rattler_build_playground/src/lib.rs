use std::str::FromStr;

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

/// Initialize the panic hook for better WASM error messages.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Parse a recipe YAML string to Stage 0 (preserving templates and conditionals).
///
/// Returns a JSON string: `{ "ok": true, "result": {...} }` or `{ "ok": false, "error": {...} }`
#[wasm_bindgen]
pub fn parse_recipe(yaml_source: &str) -> String {
    match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(recipe) => match serde_json::to_string_pretty(&recipe) {
            Ok(json) => format!(r#"{{"ok":true,"result":{json}}}"#),
            Err(e) => error_json(&e.to_string(), None, None),
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
/// Returns a JSON string: `{ "ok": true, "result": {...} }` or `{ "ok": false, "error": {...} }`
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
                Ok(stage1) => match serde_json::to_string_pretty(&stage1) {
                    Ok(json) => format!(r#"{{"ok":true,"result":{json}}}"#),
                    Err(e) => error_json(&e.to_string(), None, None),
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
                Ok(outputs) => match serde_json::to_string_pretty(&outputs) {
                    Ok(json) => format!(r#"{{"ok":true,"result":{json}}}"#),
                    Err(e) => error_json(&e.to_string(), None, None),
                },
                Err(e) => format_parse_error(&e),
            }
        }
    }
}

/// Get the list of variables used in a recipe (for UI hints).
///
/// Returns a JSON string: `{ "ok": true, "result": [...] }` or `{ "ok": false, "error": {...} }`
#[wasm_bindgen]
pub fn get_used_variables(yaml_source: &str) -> String {
    match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(recipe) => {
            let vars = match &recipe {
                Recipe::SingleOutput(r) => r.used_variables(),
                Recipe::MultiOutput(r) => r.used_variables(),
            };
            match serde_json::to_string(&vars) {
                Ok(json) => format!(r#"{{"ok":true,"result":{json}}}"#),
                Err(e) => error_json(&e.to_string(), None, None),
            }
        }
        Err(e) => format_parse_error(&e),
    }
}

/// Get available platform strings for the UI dropdown.
#[wasm_bindgen]
pub fn get_platforms() -> String {
    serde_json::to_string(&[
        "linux-64",
        "linux-aarch64",
        "linux-ppc64le",
        "linux-s390x",
        "osx-64",
        "osx-arm64",
        "win-64",
        "win-arm64",
        "noarch",
    ])
    .unwrap_or_default()
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
/// Returns a JSON string with both full data and a concise summary:
/// `{ "ok": true, "result": { "variants": [...full data...], "summary": [...concise...] } }`
#[wasm_bindgen]
pub fn render_variants(
    yaml_source: &str,
    variant_config_yaml: &str,
    target_platform: &str,
) -> String {
    // Parse the recipe to Stage 0
    let stage0_recipe = match stage0::parse_recipe_or_multi_from_source(yaml_source) {
        Ok(r) => r,
        Err(e) => return format_parse_error(&e),
    };

    // Parse platform
    let platform = Platform::from_str(target_platform).unwrap_or(Platform::Linux64);

    // Parse variant config — try conda_build_config format first (handles # [selector] syntax),
    // then fall back to the modern variants.yaml format
    let jinja_config = JinjaConfig {
        target_platform: platform,
        build_platform: platform,
        host_platform: platform,
        experimental: false,
        recipe_path: None,
        ..Default::default()
    };

    let variant_config = match parse_conda_build_config(variant_config_yaml, &jinja_config) {
        Ok(vc) => vc,
        Err(_) => match VariantConfig::from_yaml_str(variant_config_yaml) {
            Ok(vc) => vc,
            Err(e) => return error_json(&format!("Invalid variant config: {e}"), None, None),
        },
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

            // Return both full data and summary
            let full_json = serde_json::to_value(&rendered).unwrap_or_default();
            let summary_json = serde_json::to_value(&summary).unwrap_or_default();
            let result = serde_json::json!({
                "variants": full_json,
                "summary": summary_json,
            });
            match serde_json::to_string_pretty(&result) {
                Ok(json) => format!(r#"{{"ok":true,"result":{json}}}"#),
                Err(e) => error_json(&e.to_string(), None, None),
            }
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
