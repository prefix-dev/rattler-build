use std::path::{Path, PathBuf};

use fs_err as fs;
use rattler_build_recipe::stage1::{TestType, requirements::Dependency, tests::CommandsTest};
use rattler_build_script::{
    ResolvedScriptContents, Script, ScriptContent, determine_interpreter_from_path,
    platform_script_extensions,
};
use rattler_conda_types::{MatchSpec, PackageNameMatcher, Platform};

use crate::{metadata::Output, packaging::PackagingError};

/// Resolve a dependency, converting PinSubpackage/PinCompatible to concrete MatchSpecs
fn resolve_dependency(dep: &Dependency, output: &Output) -> Dependency {
    match dep {
        Dependency::Spec(_) => dep.clone(),
        Dependency::PinSubpackage(pin) => {
            let name = &pin.pin_subpackage.name;
            if let Some(subpackage) = output.build_configuration.subpackages.get(name) {
                // Apply the pin to get a concrete MatchSpec
                match pin
                    .pin_subpackage
                    .apply(&subpackage.version, &subpackage.build_string)
                {
                    Ok(spec) => Dependency::Spec(Box::new(spec)),
                    Err(_) => {
                        // If apply fails, fall back to just the package name
                        Dependency::Spec(Box::new(MatchSpec {
                            name: PackageNameMatcher::Exact(name.clone()),
                            ..Default::default()
                        }))
                    }
                }
            } else {
                // Subpackage not found, fall back to just the package name
                Dependency::Spec(Box::new(MatchSpec {
                    name: PackageNameMatcher::Exact(name.clone()),
                    ..Default::default()
                }))
            }
        }
        Dependency::PinCompatible(pin) => {
            // For pin_compatible in tests, we just use the package name
            // since we don't have compatibility_specs available here
            let name = &pin.pin_compatible.name;
            Dependency::Spec(Box::new(MatchSpec {
                name: PackageNameMatcher::Exact(name.clone()),
                ..Default::default()
            }))
        }
    }
}

/// Resolve all dependencies in a CommandsTest requirements
fn resolve_test_requirements(command_test: &mut CommandsTest, output: &Output) {
    command_test.requirements.run = command_test
        .requirements
        .run
        .iter()
        .map(|dep| resolve_dependency(dep, output))
        .collect();
    command_test.requirements.build = command_test
        .requirements
        .build
        .iter()
        .map(|dep| resolve_dependency(dep, output))
        .collect();
}

/// Store the resolved script contents back into `script`, inferring the
/// interpreter from the resolved file when it wasn't set explicitly.
fn apply_resolved_content(script: &mut Script, contents: ResolvedScriptContents) {
    match contents {
        ResolvedScriptContents::Inline(contents) => {
            script.content = ScriptContent::Command(contents);
        }
        ResolvedScriptContents::Path(path, contents) => {
            if script.interpreter.is_none() {
                script.interpreter = determine_interpreter_from_path(&path);
            }
            script.content = ScriptContent::Command(contents);
        }
        ResolvedScriptContents::Commands(commands) => {
            script.content = ScriptContent::Commands(commands);
        }
        ResolvedScriptContents::Missing => {
            script.content = ScriptContent::Command(String::new());
        }
    }
}

/// Whether the script content refers to a file on disk (as opposed to inline
/// commands). Only file-based scripts have distinct per-platform variants that
/// we need to resolve separately for noarch packages.
fn is_file_based(content: &ScriptContent) -> bool {
    match content {
        ScriptContent::Path(_) => true,
        // A bare string may be either an inline command or a path to a script;
        // a single line is treated as a potential path by `resolve_content`.
        ScriptContent::CommandOrPath(s) => !s.contains('\n'),
        _ => false,
    }
}

/// Whether a resolved script actually carries something to run.
fn script_has_content(script: &Script) -> bool {
    match &script.content {
        ScriptContent::Command(s) => !s.is_empty(),
        ScriptContent::Commands(c) => !c.is_empty(),
        _ => true,
    }
}

pub fn write_command_test_files(
    command_test: &CommandsTest,
    folder: &Path,
    output: &Output,
) -> Result<Vec<PathBuf>, PackagingError> {
    let mut test_files = Vec::new();

    if !command_test.files.recipe.is_empty() || !command_test.files.source.is_empty() {
        fs::create_dir_all(folder)?;
    }

    if !command_test.files.recipe.is_empty() {
        let globs = &command_test.files.recipe;
        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.recipe_dir,
            folder,
        )
        .with_globvec(globs)
        .use_gitignore(false)
        .run()?;

        test_files.extend(copy_dir.copied_paths().iter().cloned());
    }

    if !command_test.files.source.is_empty() {
        let globs = &command_test.files.source;
        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.work_dir,
            folder,
        )
        .with_globvec(globs)
        .use_gitignore(false)
        .run()?;

        test_files.extend(copy_dir.copied_paths().iter().cloned());
    }

    Ok(test_files)
}

/// Write out the test files for the final package
pub(crate) fn write_test_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let mut test_files = Vec::new();

    let name = output.name().as_normalized();

    // extract test section from the original recipe
    let mut tests = output.recipe.tests.clone();

    // remove the package contents tests as they are not needed in the final package
    tests.retain(|test| !matches!(test, TestType::PackageContents { .. }));

    // For each `Command` test, we need to copy the test files to the package
    for (idx, test) in tests.iter_mut().enumerate() {
        if let TestType::Commands(command_test) = test {
            // Resolve pin_subpackage dependencies to concrete MatchSpecs
            resolve_test_requirements(command_test, output);

            let cwd = PathBuf::from(format!("etc/conda/test-files/{name}/{idx}"));
            let folder = tmp_dir_path.join(&cwd);
            let files = write_command_test_files(command_test, &folder, output)?;
            if !files.is_empty() {
                test_files.extend(files);
                // store the cwd in the rendered test
                command_test.script.cwd = Some(cwd);
            }

            // Try to render the script contents here
            // TODO(refactor): properly render script here.
            let recipe_dir = &output.build_configuration.directories.recipe_dir;

            // Keep a pristine copy of the (unresolved) script so we can also
            // resolve the *other* platform's variant for noarch packages.
            let base_script = command_test.script.clone();

            // Resolve the script for the current build platform. This is the
            // primary `script` and preserves the historical behavior.
            let contents = base_script.resolve_content(
                recipe_dir,
                Some(output.jinja_renderer()),
                platform_script_extensions(),
            )?;
            // Remember which file the primary resolution came from (if any) so
            // we don't store a redundant alternate for an explicit-extension
            // script (e.g. `file: run_test.sh`).
            let primary_path = match &contents {
                ResolvedScriptContents::Path(path, _) => Some(path.clone()),
                _ => None,
            };
            apply_resolved_content(&mut command_test.script, contents);

            // For `noarch` packages the test script above was resolved on a
            // single platform, but the package may be tested on a different
            // operating system. When the test script is provided as a file,
            // also resolve the *other* platform family's script (e.g. the
            // `.bat` counterpart to a `.sh` file) and serialize it so the
            // package can be tested reliably on both Unix and Windows.
            // See https://github.com/prefix-dev/rattler-build/issues/2064.
            if output.build_configuration.target_platform == Platform::NoArch
                && is_file_based(base_script.contents())
            {
                let other_extensions: &[&str] = if cfg!(windows) {
                    &["sh"]
                } else {
                    &["bat", "ps1"]
                };

                let other = base_script.resolve_content(
                    recipe_dir,
                    Some(output.jinja_renderer()),
                    other_extensions,
                )?;

                if let ResolvedScriptContents::Path(path, script_contents) = other {
                    // Only store an alternate when it resolves to a *different*
                    // file than the primary one.
                    if primary_path.as_ref() != Some(&path) {
                        let mut alt = base_script.clone();
                        if alt.interpreter.is_none() {
                            alt.interpreter = determine_interpreter_from_path(&path);
                        }
                        alt.content = ScriptContent::Command(script_contents);
                        if cfg!(windows) {
                            command_test.script_unix = Some(alt);
                        } else {
                            command_test.script_win = Some(alt);
                        }
                    }
                }
            }
        }
    }

    // Remove command tests that resolved to empty (no script content, no
    // requirements, no test files). This happens when all commands were
    // filtered out by conditionals (e.g. `if: not build_win`).
    tests.retain(|test| {
        if let TestType::Commands(cmd) = test {
            let has_content = script_has_content(&cmd.script)
                || cmd.script_win.as_ref().is_some_and(script_has_content)
                || cmd.script_unix.as_ref().is_some_and(script_has_content);
            let has_requirements = !cmd.requirements.is_empty();
            let has_files = !cmd.files.is_empty();
            has_content || has_requirements || has_files
        } else {
            true
        }
    });

    let test_file_dir = tmp_dir_path.join("info/tests");
    fs::create_dir_all(&test_file_dir)?;

    let test_file = test_file_dir.join("tests.yaml");
    fs::write(&test_file, serde_yaml::to_string(&tests)?)?;
    test_files.push(test_file);

    Ok(test_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_file_based() {
        assert!(is_file_based(&ScriptContent::Path(PathBuf::from(
            "run_test"
        ))));
        // A single-line bare string may be a path.
        assert!(is_file_based(&ScriptContent::CommandOrPath(
            "run_test".to_string()
        )));
        // A multi-line bare string is inline commands, not a path.
        assert!(!is_file_based(&ScriptContent::CommandOrPath(
            "echo a\necho b".to_string()
        )));
        assert!(!is_file_based(&ScriptContent::Command(
            "echo hi".to_string()
        )));
        assert!(!is_file_based(&ScriptContent::Commands(vec![
            "echo hi".to_string()
        ])));
        assert!(!is_file_based(&ScriptContent::Default));
    }

    #[test]
    fn test_script_has_content() {
        assert!(script_has_content(&Script {
            content: ScriptContent::Command("echo".to_string()),
            ..Script::default()
        }));
        assert!(!script_has_content(&Script {
            content: ScriptContent::Command(String::new()),
            ..Script::default()
        }));
        assert!(!script_has_content(&Script {
            content: ScriptContent::Commands(vec![]),
            ..Script::default()
        }));
        assert!(script_has_content(&Script {
            content: ScriptContent::Commands(vec!["echo".to_string()]),
            ..Script::default()
        }));
    }

    #[test]
    fn test_apply_resolved_content_infers_interpreter() {
        let mut script = Script::default();
        apply_resolved_content(
            &mut script,
            ResolvedScriptContents::Path(PathBuf::from("run_test.bat"), "echo hi".to_string()),
        );
        assert_eq!(script.interpreter.as_deref(), Some("cmd"));
        assert_eq!(
            script.content,
            ScriptContent::Command("echo hi".to_string())
        );

        // An explicit interpreter is preserved.
        let mut script = Script {
            interpreter: Some("nushell".to_string()),
            ..Script::default()
        };
        apply_resolved_content(
            &mut script,
            ResolvedScriptContents::Path(PathBuf::from("run_test.sh"), "echo hi".to_string()),
        );
        assert_eq!(script.interpreter.as_deref(), Some("nushell"));
    }
}
