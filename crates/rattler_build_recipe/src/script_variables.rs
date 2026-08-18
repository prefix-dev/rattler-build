//! Auto-detection of variant variables referenced by build scripts.
//!
//! Recipes often consume variant variables directly from the environment of
//! their build script (e.g. `${TARGET}` in `build.sh` or `%TARGET%` in
//! `build.bat`) without referencing them anywhere in the recipe itself.
//! Historically such variables had to be forwarded manually via
//! `build.variant.use_keys`.
//!
//! This module walks the build scripts of a recipe (inline content, explicit
//! script files, and the default `build.sh` / `build.bat` discovery) and
//! checks which variant configuration keys occur literally in the script
//! text (see [`rattler_build_script::variable_scan`]). The search is
//! deliberately interpreter-agnostic: any word-bounded, case-sensitive
//! occurrence of a key name counts as a usage.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rattler_build_script::{Script as ExecutableScript, ScriptContent};

use crate::stage0::{
    self, BuildPlan as Stage0BuildPlan, Item, NestedItemList, Recipe as Stage0Recipe,
    Script as Stage0Script, Step as Stage0Step,
};
use crate::stage1::{BuildPlan as Stage1BuildPlan, Recipe as Stage1Recipe};

/// Collect all *concrete* strings from a conditional list, including both
/// branches of conditionals. Templates are skipped: variables they reference
/// are already tracked by the regular `used_variables` machinery, and their
/// rendered output cannot be scanned before evaluation.
fn collect_concrete_strings<'a>(
    items: impl Iterator<Item = &'a Item<String>>,
    out: &mut Vec<String>,
) {
    for item in items {
        match item {
            Item::Value(value) => {
                if let Some(concrete) = value.as_concrete() {
                    out.push(concrete.clone());
                }
            }
            Item::Conditional(conditional) => {
                collect_nested(&conditional.then, out);
                if let Some(else_value) = &conditional.else_value {
                    collect_nested(else_value, out);
                }
            }
        }
    }
}

fn collect_nested(items: &NestedItemList<String>, out: &mut Vec<String>) {
    collect_concrete_strings(items.iter(), out);
}

/// Convert a stage0 script into an executable-script representation that only
/// contains the concrete (non-template) parts, mirroring the content shapes
/// produced by stage0 evaluation.
fn stage0_script_to_executable(script: &Stage0Script) -> ExecutableScript {
    let content = if let Some(file) = script.file.as_ref().and_then(|value| value.as_concrete()) {
        ScriptContent::Path(PathBuf::from(file))
    } else if let Some(content) = &script.content {
        let mut commands = Vec::new();
        collect_concrete_strings(content.iter(), &mut commands);
        if commands.len() == 1 && !script.content_explicit {
            // A single plain string can be either a command or a script file
            // reference (`script: install.sh`).
            ScriptContent::CommandOrPath(commands.remove(0))
        } else {
            ScriptContent::Commands(commands)
        }
    } else {
        ScriptContent::Default
    };

    ExecutableScript {
        content,
        ..Default::default()
    }
}

/// Collect the scannable scripts of a stage0 build plan.
///
/// `allow_default_discovery` controls whether a default (empty) script maps to
/// the implicit `build.sh` / `build.bat` lookup: that discovery only exists
/// for single-output recipes.
fn stage0_plan_scripts(
    plan: &Stage0BuildPlan,
    allow_default_discovery: bool,
    out: &mut Vec<ExecutableScript>,
) {
    match plan {
        Stage0BuildPlan::Script(script) => {
            let executable = stage0_script_to_executable(script);
            if executable.content.is_default() && !allow_default_discovery {
                return;
            }
            out.push(executable);
        }
        Stage0BuildPlan::Steps(steps) => {
            for step in steps {
                let Stage0Step::Run(run) = step;
                let mut commands = Vec::new();
                collect_concrete_strings(run.run.iter(), &mut commands);
                out.push(ExecutableScript {
                    // Steps are always inline; they never reference files.
                    content: ScriptContent::Commands(commands),
                    ..Default::default()
                });
            }
        }
    }
}

/// Detect which of the `candidates` (variant configuration keys, in their
/// normalized spelling) are referenced by any build script of a stage0 recipe
/// (before evaluation). Used to seed the variant combination matrix.
pub fn stage0_script_variables(
    recipe: &Stage0Recipe,
    candidates: &[String],
    recipe_dir: Option<&Path>,
    is_windows: bool,
) -> BTreeSet<String> {
    let mut scripts = Vec::new();
    match recipe {
        Stage0Recipe::SingleOutput(single) => {
            stage0_plan_scripts(&single.build.plan, true, &mut scripts);
        }
        Stage0Recipe::MultiOutput(multi) => {
            // Multi-output recipes never auto-discover `build.sh`/`build.bat`.
            stage0_plan_scripts(&multi.build.plan, false, &mut scripts);
            for output in &multi.outputs {
                match output {
                    stage0::Output::Staging(staging) => {
                        stage0_plan_scripts(&staging.build.plan, false, &mut scripts);
                    }
                    stage0::Output::Package(package) => {
                        stage0_plan_scripts(&package.build.plan, false, &mut scripts);
                    }
                }
            }
        }
    }

    let mut variables = BTreeSet::new();
    for script in scripts {
        variables.extend(script.detect_used_variables(candidates, recipe_dir, is_windows));
    }
    variables
}

/// Detect which of the `candidates` (variant configuration keys, in their
/// normalized spelling) are referenced by the build script of a single
/// evaluated (stage1) output. Used to record script-referenced variant
/// variables in the output's variant so they participate in the build hash.
pub fn stage1_script_variables(
    recipe: &Stage1Recipe,
    candidates: &[String],
    recipe_dir: Option<&Path>,
    is_windows: bool,
) -> BTreeSet<String> {
    match &recipe.build.plan {
        Stage1BuildPlan::Script(script) => {
            script.detect_used_variables(candidates, recipe_dir, is_windows)
        }
        Stage1BuildPlan::Steps(steps) => {
            let mut variables = BTreeSet::new();
            for step in steps {
                variables.extend(
                    step.to_script()
                        .detect_used_variables(candidates, recipe_dir, is_windows),
                );
            }
            variables
        }
    }
}
