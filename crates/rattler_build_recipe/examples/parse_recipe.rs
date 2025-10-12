//! Example binary to parse and evaluate a recipe YAML file
//!
//! Usage:
//!   cargo run --example parse_recipe <recipe.yaml> [key=value ...]
//!
//! Examples:
//!   cargo run --example parse_recipe recipe.yaml
//!   cargo run --example parse_recipe recipe.yaml name=foo version=1.0.0
//!   cargo run --example parse_recipe recipe.yaml name=bar version=2.0 unix=true

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

use miette::{IntoDiagnostic, NamedSource, Result};
use rattler_build_recipe::{Evaluate, EvaluationContext, stage0};

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

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <recipe.yaml> [key=value ...]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} recipe.yaml", args[0]);
        eprintln!("  {} recipe.yaml name=foo version=1.0.0", args[0]);
        eprintln!("  {} recipe.yaml name=bar version=2.0 unix=true", args[0]);
        process::exit(1);
    }

    let recipe_path = &args[1];

    // Parse variable arguments (key=value pairs)
    let mut variables = HashMap::new();
    for arg in &args[2..] {
        if let Some((key, value)) = arg.split_once('=') {
            variables.insert(key.to_string(), value.to_string());
        } else {
            eprintln!(
                "Warning: ignoring invalid argument '{}' (expected key=value)",
                arg
            );
        }
    }

    // Read the recipe file
    let yaml_content = fs::read_to_string(recipe_path).into_diagnostic()?;

    println!("=== Parsing recipe: {} ===\n", recipe_path);

    // Create a named source for better error messages with miette
    let source = NamedSource::new(recipe_path, yaml_content.clone());

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
    let known_functions = vec![
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
    println!(
        "Package: {} {}",
        stage1_recipe.package().name().as_normalized(),
        stage1_recipe.package().version()
    );

    if let Some(homepage) = &stage1_recipe.about().homepage {
        println!("Homepage: {}", homepage);
    }

    if let Some(license) = &stage1_recipe.about().license {
        println!("License: {}", license.as_ref());
    }

    if !stage1_recipe.requirements().build.is_empty() {
        println!("\nBuild requirements:");
        for dep in &stage1_recipe.requirements().build {
            println!("  - {}", dep);
        }
    }

    if !stage1_recipe.requirements().host.is_empty() {
        println!("\nHost requirements:");
        for dep in &stage1_recipe.requirements().host {
            println!("  - {}", dep);
        }
    }

    if !stage1_recipe.requirements().run.is_empty() {
        println!("\nRun requirements:");
        for dep in &stage1_recipe.requirements().run {
            println!("  - {}", dep);
        }
    }

    if !stage1_recipe.extra().recipe_maintainers.is_empty() {
        println!("\nMaintainers:");
        for maintainer in &stage1_recipe.extra().recipe_maintainers {
            println!("  - {}", maintainer);
        }
    }

    println!("\n=== Complete Stage1 Recipe ===");
    println!("{:#?}", stage1_recipe);

    Ok(())
}
