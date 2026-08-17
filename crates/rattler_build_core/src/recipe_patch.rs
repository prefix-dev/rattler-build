//! Post-build recipe metadata updates emitted by build steps.

use std::path::{Path, PathBuf};

use json_patch::{Patch, PatchOperation};
use miette::{IntoDiagnostic, WrapErr};
use rattler_build_recipe::stage1::Recipe;
use serde_json::{Map, Value};

const OUTPUT_DIRECTORY: &str = ".rattler-build/step-outputs";

/// Directory containing the RFC 6902 JSON Patch files emitted by build steps.
pub(crate) fn output_directory(work_dir: &Path) -> PathBuf {
    work_dir.join(OUTPUT_DIRECTORY)
}

/// Path exposed to a particular build-script section as `OUTPUT_FILE`.
pub(crate) fn output_file(work_dir: &Path, index: usize) -> PathBuf {
    output_directory(work_dir).join(format!("{index:06}.json"))
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

fn validate_patch(patch: &Patch, source: &Path) -> miette::Result<()> {
    for operation in &patch.0 {
        let path = operation.path().to_string();
        if !allowed_path(&path) {
            return Err(miette::miette!(
                "recipe patch {} cannot modify `{path}` after the build environment has been solved",
                source.display()
            ));
        }
        let from = match operation {
            PatchOperation::Move(operation) => Some(operation.from.to_string()),
            PatchOperation::Copy(operation) => Some(operation.from.to_string()),
            _ => None,
        };
        if let Some(from) = from
            && !allowed_path(&from)
        {
            return Err(miette::miette!(
                "recipe patch {} cannot read `{from}` outside the post-build mutable fields",
                source.display()
            ));
        }
    }
    Ok(())
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
}

fn apply_patch_file(recipe: &mut Recipe, path: &Path) -> miette::Result<()> {
    let contents = fs_err::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read recipe patch {}", path.display()))?;
    let patch: Patch = serde_json::from_str(&contents)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "failed to parse recipe patch {} as RFC 6902 JSON Patch",
                path.display()
            )
        })?;
    validate_patch(&patch, path)?;

    let used_variant = recipe.used_variant.clone();
    let mut document = serde_json::to_value(&*recipe)
        .into_diagnostic()
        .wrap_err("failed to serialize recipe before applying build-step output")?;
    normalize_patch_document(&mut document);
    json_patch::patch(&mut document, &patch)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to apply recipe patch {}", path.display()))?;
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

/// Apply all step outputs in deterministic execution order.
pub(crate) fn apply_outputs(recipe: &mut Recipe, work_dir: &Path) -> miette::Result<()> {
    let directory = output_directory(work_dir);
    if !directory.is_dir() {
        return Ok(());
    }
    let mut outputs = fs_err::read_dir(&directory)
        .into_diagnostic()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    outputs.sort();
    for output in outputs {
        apply_patch_file(recipe, &output)?;
    }
    Ok(())
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
            r#"[
  {"op":"add","path":"/about/license_file/include/-","value":"/generated/LICENSE*"},
  {"op":"replace","path":"/build/dynamic_linking/rpaths","value":["lib/","lib/custom"]}
]"#,
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
    fn rejects_fields_consumed_before_build_execution() {
        let temp = tempfile::tempdir().unwrap();
        prepare_output_directory(temp.path()).unwrap();
        fs_err::write(
            output_file(temp.path(), 0),
            r#"[{"op":"replace","path":"/requirements/build","value":[]}]"#,
        )
        .unwrap();
        let mut recipe = recipe();

        let error = apply_outputs(&mut recipe, temp.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot modify `/requirements/build`")
        );
    }
}
