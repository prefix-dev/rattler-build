//! Post-build recipe metadata updates emitted by build steps.

use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, WrapErr};
use rattler_build_recipe::stage1::{Dependency, Recipe, requirements::RunExports};
use serde_json::{Map, Value};

const OUTPUT_DIRECTORY: &str = ".rattler-build/step-outputs";

/// Directory containing metadata output files emitted by build steps.
pub(crate) fn output_directory(work_dir: &Path) -> PathBuf {
    work_dir.join(OUTPUT_DIRECTORY)
}

/// Path exposed to a particular build-script section as `OUTPUT_FILE`.
pub(crate) fn output_file(work_dir: &Path, index: usize) -> PathBuf {
    output_directory(work_dir).join(format!("{index:06}.txt"))
}

/// Remove stale outputs and create the output directory before script execution.
pub(crate) fn prepare_output_directory(work_dir: &Path) -> std::io::Result<()> {
    let directory = output_directory(work_dir);
    match fs_err::remove_dir_all(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs_err::create_dir_all(directory)
}

fn allowed_path(path: &str) -> bool {
    path.starts_with("/about/")
        || path == "/requirements/run"
        || path.starts_with("/requirements/run/")
        || path == "/requirements/run_constraints"
        || path.starts_with("/requirements/run_constraints/")
        || path == "/requirements/run_exports"
        || path.starts_with("/requirements/run_exports/")
        || path == "/build/dynamic_linking"
        || path.starts_with("/build/dynamic_linking/")
        || path == "/build/prefix_detection"
        || path.starts_with("/build/prefix_detection/")
        || [
            "/build/files",
            "/build/always_copy_files",
            "/build/always_include_files",
            "/build/post_process",
        ]
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
}

fn object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value was replaced by an object")
}

fn ensure_object<'a>(value: &'a mut Value, path: &[&str]) -> &'a mut Value {
    let mut current = value;
    for segment in path {
        current = object(current)
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current
}

fn ensure_array(value: &mut Value, parent: &[&str], key: &str) {
    object(ensure_object(value, parent))
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
}

/// Normalize include/exclude glob collections to their map representation so
/// their include arrays always have a stable JSON Pointer, even when excludes
/// were present in the original recipe.
fn normalize_globs(value: &mut Value, parent: &[&str], key: &str) {
    let entry = object(ensure_object(value, parent))
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let old = std::mem::take(entry);
    *entry = match old {
        Value::Array(include) => serde_json::json!({"include": include, "exclude": []}),
        Value::Object(mut globs) => {
            globs
                .entry("include".to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            globs
                .entry("exclude".to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            Value::Object(globs)
        }
        value => value,
    };
}

/// Materialize commonly extended collections with stable JSON Pointer paths.
fn normalize_patch_document(document: &mut Value) {
    ensure_object(document, &["about"]);
    normalize_globs(document, &["about"], "license_file");

    ensure_object(document, &["build", "dynamic_linking"]);
    ensure_array(document, &["build", "dynamic_linking"], "rpaths");
    for key in ["missing_dso_allowlist", "rpath_allowlist"] {
        normalize_globs(document, &["build", "dynamic_linking"], key);
    }
    ensure_object(document, &["build", "prefix_detection"]);
    for key in ["files", "always_copy_files", "always_include_files"] {
        normalize_globs(document, &["build"], key);
    }
    ensure_array(document, &["build"], "post_process");

    for key in ["run", "run_constraints"] {
        ensure_array(document, &["requirements"], key);
    }
    for key in [
        "noarch",
        "strong",
        "strong_constraints",
        "weak",
        "weak_constraints",
    ] {
        ensure_array(document, &["requirements", "run_exports"], key);
    }
}

fn parse_output_value(raw: &str) -> Value {
    if matches!(raw.as_bytes().first(), Some(b'[' | b'{' | b'"')) {
        serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
    } else {
        Value::String(raw.to_string())
    }
}

fn apply_text_output(document: &mut Value, contents: &str, source: &Path) -> miette::Result<()> {
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (directive, raw_value) = line.split_once(char::is_whitespace).ok_or_else(|| {
            miette::miette!(
                "invalid build-step output {} line {}: expected `PATH VALUE`",
                source.display(),
                index + 1
            )
        })?;
        let (dotted_path, append) = directive
            .strip_suffix(".append")
            .map_or((directive, false), |path| (path, true));
        let pointer = format!("/{}", dotted_path.replace('.', "/"));
        if dotted_path == "requirements.build" || dotted_path == "requirements.host" {
            return Err(miette::miette!(
                "build-step output {} cannot add `{dotted_path}` after environments have been solved; declare build/host requirements on the reusable step",
                source.display()
            ));
        }
        if !allowed_path(&pointer) {
            return Err(miette::miette!(
                "build-step output {} cannot modify `{dotted_path}` after build execution",
                source.display()
            ));
        }
        if pointer.starts_with("/requirements/") && !append {
            return Err(miette::miette!(
                "build-step output {} must use `.append` for post-build requirements",
                source.display()
            ));
        }

        let value = parse_output_value(raw_value.trim());
        if append {
            let target = document.pointer_mut(&pointer).ok_or_else(|| {
                miette::miette!(
                    "build-step output {} cannot append to unknown collection `{dotted_path}`",
                    source.display()
                )
            })?;
            let array = target.as_array_mut().ok_or_else(|| {
                miette::miette!(
                    "build-step output {} target `{dotted_path}` is not a collection",
                    source.display()
                )
            })?;
            match value {
                Value::Array(values) => array.extend(values),
                value => array.push(value),
            }
        } else {
            let (parent_path, key) = pointer.rsplit_once('/').expect("pointer starts with slash");
            let parent = document.pointer_mut(parent_path).ok_or_else(|| {
                miette::miette!(
                    "build-step output {} has unknown parent for `{dotted_path}`",
                    source.display()
                )
            })?;
            object(parent).insert(key.to_string(), value);
        }
    }
    Ok(())
}

fn apply_patch_file(recipe: &mut Recipe, path: &Path) -> miette::Result<()> {
    let contents = fs_err::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read recipe patch {}", path.display()))?;
    let used_variant = recipe.used_variant.clone();
    let mut document = serde_json::to_value(&*recipe)
        .into_diagnostic()
        .wrap_err("failed to serialize recipe before applying build-step output")?;
    normalize_patch_document(&mut document);
    apply_text_output(&mut document, &contents, path)?;
    let mut updated: Recipe = serde_json::from_value(document)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "recipe patch {} produced invalid recipe metadata",
                path.display()
            )
        })?;
    updated.used_variant = used_variant;
    *recipe = updated;
    Ok(())
}

/// Requirements that were appended after the build completed.
#[derive(Debug, Default)]
pub(crate) struct PostBuildRequirements {
    pub(crate) run: Vec<Dependency>,
    pub(crate) run_constraints: Vec<Dependency>,
    pub(crate) run_exports: RunExports,
}

/// Apply all step outputs in deterministic execution order.
pub(crate) fn apply_outputs(
    recipe: &mut Recipe,
    work_dir: &Path,
) -> miette::Result<PostBuildRequirements> {
    let directory = output_directory(work_dir);
    if !directory.is_dir() {
        return Ok(PostBuildRequirements::default());
    }
    let original_run = recipe.requirements.run.len();
    let original_constraints = recipe.requirements.run_constraints.len();
    let original_exports = recipe.requirements.run_exports.clone();
    let mut outputs = fs_err::read_dir(&directory)
        .into_diagnostic()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    outputs.sort();
    for output in outputs {
        apply_patch_file(recipe, &output)?;
    }
    let exports = &recipe.requirements.run_exports;
    Ok(PostBuildRequirements {
        run: recipe.requirements.run[original_run..].to_vec(),
        run_constraints: recipe.requirements.run_constraints[original_constraints..].to_vec(),
        run_exports: RunExports {
            noarch: exports.noarch[original_exports.noarch.len()..].to_vec(),
            strong: exports.strong[original_exports.strong.len()..].to_vec(),
            strong_constraints: exports.strong_constraints
                [original_exports.strong_constraints.len()..]
                .to_vec(),
            weak: exports.weak[original_exports.weak.len()..].to_vec(),
            weak_constraints: exports.weak_constraints[original_exports.weak_constraints.len()..]
                .to_vec(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_build_recipe::stage1::{About, Build, Extra, Package, Requirements};
    use rattler_conda_types::PackageName;

    fn recipe() -> Recipe {
        Recipe::new(
            Package::new(
                PackageName::new_unchecked("patch-test"),
                "1.0.0".parse().unwrap(),
            ),
            Build::default(),
            About::default(),
            Requirements::default(),
            Extra::default(),
            Vec::new(),
            Vec::new(),
            Default::default(),
            Default::default(),
        )
    }

    #[test]
    fn applies_standard_array_append_and_build_replacement() {
        let temp = tempfile::tempdir().unwrap();
        prepare_output_directory(temp.path()).unwrap();
        fs_err::write(
            output_file(temp.path(), 0),
            "about.license_file.include.append /generated/LICENSE*\nbuild.dynamic_linking.rpaths [\"lib/\",\"lib/custom\"]\n",
        )
        .unwrap();
        let mut recipe = recipe();
        recipe.about.license_file = Some(
            rattler_build_types::LateBoundGlobVec::from_sources(
                vec!["LICENSE".to_string()],
                vec!["LICENSE.private".to_string()],
            )
            .unwrap(),
        );

        apply_outputs(&mut recipe, temp.path()).unwrap();

        assert_eq!(
            recipe
                .about
                .license_file
                .as_ref()
                .unwrap()
                .entries()
                .iter()
                .map(|entry| entry.source())
                .collect::<Vec<_>>(),
            ["LICENSE", "/generated/LICENSE*"]
        );
        assert_eq!(
            recipe.about.license_file.as_ref().unwrap().exclude(),
            ["LICENSE.private"]
        );
        assert_eq!(
            recipe.build.dynamic_linking.rpaths.to_vec(),
            ["lib/", "lib/custom"]
        );
    }

    #[test]
    fn applies_cat_friendly_metadata_and_requirement_outputs() {
        let temp = tempfile::tempdir().unwrap();
        prepare_output_directory(temp.path()).unwrap();
        fs_err::write(
            output_file(temp.path(), 0),
            r#"# plain values are strings; arrays use JSON
about.repository https://example.com/source
about.summary generated by a reusable step
requirements.run.append ["runtime >=1", "helper"]
requirements.run_exports.strong.append ["abi >=2"]
"#,
        )
        .unwrap();
        let mut recipe = recipe();

        let changes = apply_outputs(&mut recipe, temp.path()).unwrap();

        assert_eq!(
            recipe.about.repository.as_ref().unwrap().as_str(),
            "https://example.com/source"
        );
        assert_eq!(
            recipe.about.summary.as_deref(),
            Some("generated by a reusable step")
        );
        assert_eq!(changes.run.len(), 2);
        assert_eq!(changes.run_exports.strong.len(), 1);
        assert_eq!(recipe.requirements.run.len(), 2);
    }

    #[test]
    fn rejects_fields_consumed_before_build_execution() {
        let temp = tempfile::tempdir().unwrap();
        prepare_output_directory(temp.path()).unwrap();
        fs_err::write(
            output_file(temp.path(), 0),
            "requirements.build.append [\"too-late\"]\n",
        )
        .unwrap();
        let mut recipe = recipe();

        let error = apply_outputs(&mut recipe, temp.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("after environments have been solved")
        );

        prepare_output_directory(temp.path()).unwrap();
        fs_err::write(
            output_file(temp.path(), 0),
            "requirements.host.append [\"too-late\"]\n",
        )
        .unwrap();
        let error = apply_outputs(&mut recipe, temp.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("after environments have been solved")
        );
    }
}
