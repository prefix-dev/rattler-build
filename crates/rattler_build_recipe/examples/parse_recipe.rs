//! Example binary to parse and evaluate a recipe YAML file
//!
//! # Usage
//!
//! **Easiest** - Use the cargo alias (automatically enables all features):
//! ```bash
//! cargo example parse_recipe -- <recipe.yaml> [OPTIONS]
//! ```
//!
//! Or run with all features enabled manually:
//! ```bash
//! cargo run --example parse_recipe --all-features -- <recipe.yaml> [OPTIONS]
//! ```
//!
//! # Examples
//!
//! ```bash
//! # Parse a simple recipe
//! cargo example parse_recipe -- recipe.yaml
//!
//! # Define context variables
//! cargo example parse_recipe -- recipe.yaml -Dname=foo -Dversion=1.0.0
//!
//! # Use variant configuration
//! cargo example parse_recipe -- recipe.yaml --variants variants.yaml
//!
//! # Combine variants with extra context
//! cargo example parse_recipe -- recipe.yaml --variants variants.yaml -Dunix=true
//! ```

use clap::Parser;
use indexmap::IndexMap;
use miette::{IntoDiagnostic, NamedSource, Result};
use rattler_build_recipe::variant_render::{RenderConfig, render_recipe_with_variants};
use rattler_build_recipe::{Evaluate, EvaluationContext, stage0};

#[derive(Parser)]
#[command(name = "parse_recipe")]
#[command(about = "Parse and evaluate a recipe YAML file", long_about = None)]
struct Args {
    /// Path to the recipe YAML file
    recipe: String,

    /// Path to variant configuration file (e.g., variants.yaml)
    #[arg(short, long)]
    variants: Option<String>,

    /// Define context variables (can be used multiple times)
    /// Format: key=value
    #[arg(short = 'D', long = "define", value_name = "KEY=VALUE")]
    variables: Vec<String>,
}

fn main() -> Result<()> {
    // Install miette panic handler for better error messages
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .build(),
        )
    }))
    .ok();

    let args = Args::parse();

    // Parse key=value variables
    let mut variables = IndexMap::new();
    for var in &args.variables {
        if let Some((key, value)) = var.split_once('=') {
            variables.insert(key.to_string(), value.to_string());
        } else {
            eprintln!(
                "Warning: ignoring invalid variable '{}' (expected key=value)",
                var
            );
        }
    }

    // Check if we should use variant rendering
    if let Some(ref variant_file) = args.variants {
        return render_with_variants(&args.recipe, variant_file, variables);
    }

    // Read the recipe file
    let yaml_content = fs_err::read_to_string(&args.recipe).into_diagnostic()?;

    println!("=== Parsing recipe: {} ===\n", args.recipe);

    // Create a named source for better error messages with miette
    let source = NamedSource::new(&args.recipe, yaml_content.clone());

    // Parse stage0 recipe
    let stage0_recipe = stage0::parse_recipe_from_source(&yaml_content)
        .map_err(|e| miette::Report::new(e).with_source_code(source.clone()))?;

    println!("✓ Stage0 recipe parsed successfully");
    println!("\n=== Stage0 Recipe (with templates and conditionals) ===");
    println!("{}", serde_json::to_string_pretty(&stage0_recipe).unwrap());

    // Collect variables used in the recipe
    let used_vars = stage0_recipe.used_variables();
    if !used_vars.is_empty() {
        println!("\n=== Variables used in recipe ===");
        for var in &used_vars {
            println!("  - {}", var);
        }
    }

    // Create evaluation context
    let mut context = EvaluationContext::from_map(variables.clone());

    // Evaluate and merge the recipe's context section
    if !stage0_recipe.context.is_empty() {
        println!("\n=== Evaluating context section ===");
        for (key, value) in &stage0_recipe.context {
            println!("  {} = {}", key, value);
        }

        context = context
            .with_context(&stage0_recipe.context)
            .map_err(|e| miette::Report::new(e).with_source_code(source.clone()))?;

        println!("\n✓ Context evaluated successfully");
    }

    // Show evaluation context
    if !variables.is_empty() || !stage0_recipe.context.is_empty() {
        println!("\n=== Final evaluation context ===");
        for (key, value) in context.variables() {
            println!("  {} = {}", key, value);
        }
    }

    // Check for missing variables (excluding known Jinja function names)
    let known_functions = [
        "compiler",
        "cdt",
        "match",
        "is_linux",
        "is_osx",
        "is_windows",
        "is_unix",
    ];
    let missing_vars: Vec<_> = used_vars
        .iter()
        .filter(|v| !context.contains(v) && !known_functions.contains(&v.as_str()))
        .collect();

    if !missing_vars.is_empty() {
        println!("\n⚠️  Warning: Missing variables:");
        for var in &missing_vars {
            println!("  - {}", var);
        }
        println!("\nThese variables will be treated as undefined in the evaluation.");
    }

    // Evaluate to stage1
    println!("\n=== Evaluating recipe ===");
    let stage1_recipe = stage0_recipe
        .evaluate(&context)
        .map_err(|e| miette::Report::new(e).with_source_code(source.clone()))?;

    println!("✓ Recipe evaluated successfully");

    // Show which variables were actually accessed during evaluation
    let accessed_vars = context.accessed_variables();
    if !accessed_vars.is_empty() {
        println!("\n=== Variables accessed during evaluation ===");
        let mut sorted_accessed: Vec<_> = accessed_vars.iter().collect();
        sorted_accessed.sort();
        for var in sorted_accessed {
            let defined = context.contains(var);
            let status = if defined { "✓" } else { "✗ (undefined)" };
            println!("  {} {}", status, var);
        }

        // Show which defined variables were NOT accessed (might be in conditional branches not taken)
        let unused_vars: Vec<_> = variables
            .keys()
            .filter(|k| !accessed_vars.contains(k.as_str()))
            .collect();

        if !unused_vars.is_empty() {
            println!("\n=== Defined variables NOT accessed ===");
            println!("(These may be in conditional branches that were not taken)");
            for var in unused_vars {
                println!("  - {}", var);
            }
        }
    } else {
        println!("\n=== No template variables were accessed ===");
        println!("(Recipe contains only concrete values, no templates were rendered)");
    }

    println!("\n=== Stage1 Recipe (evaluated with concrete types) ===");
    println!("{}", serde_yaml::to_string(&stage1_recipe).unwrap());

    Ok(())
}

fn render_with_variants(
    recipe_path: &str,
    variant_file: &str,
    extra_context: IndexMap<String, String>,
) -> Result<()> {
    use std::path::Path;

    println!("=== Rendering recipe with variants ===");
    println!("Recipe: {}", recipe_path);
    println!("Variants: {}", variant_file);

    if !extra_context.is_empty() {
        println!("\nExtra context:");
        for (key, value) in &extra_context {
            println!("  {} = {}", key, value);
        }
    }

    // Create render config
    let mut config = RenderConfig::new();
    for (key, value) in extra_context {
        config = config.with_context(key, value);
    }

    // Render the recipe with all variant combinations
    let rendered = render_recipe_with_variants(
        Path::new(recipe_path),
        &[Path::new(variant_file)],
        Some(config),
    )
    .into_diagnostic()?;

    println!(
        "\n=== Found {} variant combination(s) ===\n",
        rendered.len()
    );

    // Display each variant
    for (idx, variant_result) in rendered.iter().enumerate() {
        println!("╔══════════════════════════════════════════════════════════════");
        println!("║ Variant #{}", idx + 1);
        println!("╚══════════════════════════════════════════════════════════════");

        if !variant_result.variant.is_empty() {
            println!("\n📦 Variant values:");
            for (key, value) in &variant_result.variant {
                println!("  {} = {}", key.normalize(), value);
            }
        } else {
            println!("\n(No variant values - using defaults)");
        }

        let recipe = &variant_result.recipe;

        println!("\n📋 Package:");
        println!("  Name:    {}", recipe.package().name().as_normalized());
        println!("  Version: {}", recipe.package().version());

        if !recipe.requirements().build.is_empty() {
            println!("\n🔨 Build requirements:");
            for dep in &recipe.requirements().build {
                println!("  - {}", dep);
            }
        }

        if !recipe.requirements().host.is_empty() {
            println!("\n🏠 Host requirements:");
            for dep in &recipe.requirements().host {
                println!("  - {}", dep);
            }
        }

        if !recipe.requirements().run.is_empty() {
            println!("\n🏃 Run requirements:");
            for dep in &recipe.requirements().run {
                println!("  - {}", dep);
            }
        }

        if let Some(homepage) = &recipe.about().homepage {
            println!("\n🌐 Homepage: {}", homepage);
        }

        if let Some(license) = &recipe.about().license {
            println!("📄 License: {}", license);
        }

        println!();
    }

    println!("✓ Successfully rendered {} variant(s)", rendered.len());

    Ok(())
}
