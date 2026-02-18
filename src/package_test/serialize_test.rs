use std::path::{Path, PathBuf};

use fs_err as fs;
use rattler_build_recipe::stage1::{TestType, requirements::Dependency, tests::CommandsTest};
use rattler_build_script::{
    ResolvedScriptContents, ScriptContent, determine_interpreter_from_path,
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
                            name: Some(PackageNameMatcher::Exact(name.clone())),
                            ..Default::default()
                        }))
                    }
                }
            } else {
                // Subpackage not found, fall back to just the package name
                Dependency::Spec(Box::new(MatchSpec {
                    name: Some(PackageNameMatcher::Exact(name.clone())),
                    ..Default::default()
                }))
            }
        }
        Dependency::PinCompatible(pin) => {
            // For pin_compatible in tests, we just use the package name
            // since we don't have compatibility_specs available here
            let name = &pin.pin_compatible.name;
            Dependency::Spec(Box::new(MatchSpec {
                name: Some(PackageNameMatcher::Exact(name.clone())),
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
            let jinja_renderer = output.jinja_renderer();

            let is_noarch =
                output.build_configuration.target_platform == Platform::NoArch;

            // For noarch packages with Commands content, keep the commands list
            // intact so they can be properly resolved at test time on any
            // platform. This ensures that platform-specific modifications (like
            // adding `if %errorlevel%` checks on Windows) happen at test time
            // rather than build time, making the package work correctly on both
            // Unix and Windows.
            if is_noarch {
                if let ScriptContent::Commands(commands) = &command_test.script.content
                {
                    let rendered_commands: Vec<String> = commands
                        .iter()
                        .map(|cmd| jinja_renderer(cmd))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| {
                            std::io::Error::other(format!(
                                "Failed to render jinja template in test script: {}",
                                e
                            ))
                        })?;
                    command_test.script.content =
                        ScriptContent::Commands(rendered_commands);
                    continue;
                }
            }

            let contents = command_test.script.resolve_content(
                &output.build_configuration.directories.recipe_dir,
                Some(&jinja_renderer),
                &["sh", "bat"],
            )?;

            // For noarch packages with file-based scripts (Path, CommandOrPath
            // resolving to a file), try to also find the other platform's
            // script variant. This allows the test to use the correct script
            // on each platform.
            if is_noarch {
                if let ResolvedScriptContents::Path(ref found_path, _) = contents {
                    // Determine which platform variant we found
                    let ext = found_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");

                    // Try to find the other platform's variant
                    let other_ext = match ext {
                        "sh" | "bash" => Some("bat"),
                        "bat" | "cmd" => Some("sh"),
                        _ => None,
                    };

                    if let Some(other_ext) = other_ext {
                        let other_path = found_path.with_extension(other_ext);
                        if other_path.is_file() {
                            let other_contents =
                                fs::read_to_string(&other_path)?;
                            let other_contents = jinja_renderer(&other_contents)
                                .map_err(|e| {
                                    std::io::Error::other(format!(
                                        "Failed to render jinja template in test script: {}",
                                        e
                                    ))
                                })?;

                            // Store the Windows variant separately
                            if ext == "sh" || ext == "bash" {
                                command_test.script.content_windows =
                                    Some(other_contents);
                            } else {
                                // The found file is the Windows variant;
                                // store the Unix variant as the main content
                                // and set the Windows variant from the
                                // originally found file.
                                let main_contents = contents.script().to_string();
                                let main_contents =
                                    jinja_renderer(&main_contents).map_err(|e| {
                                        std::io::Error::other(format!(
                                            "Failed to render jinja template in test script: {}",
                                            e
                                        ))
                                    })?;
                                command_test.script.content =
                                    ScriptContent::Command(other_contents);
                                command_test.script.content_windows =
                                    Some(main_contents);
                                command_test.script.interpreter =
                                    Some("bash".to_string());
                                continue;
                            }
                        }
                    }
                }
            }

            // Replace with rendered contents
            match contents {
                ResolvedScriptContents::Inline(contents) => {
                    command_test.script.content = ScriptContent::Command(contents)
                }
                ResolvedScriptContents::Path(path, contents) => {
                    if command_test.script.interpreter.is_none() {
                        command_test.script.interpreter = determine_interpreter_from_path(&path);
                    }
                    command_test.script.content = ScriptContent::Command(contents);
                }
                ResolvedScriptContents::Missing => {
                    command_test.script.content = ScriptContent::Command("".to_string());
                }
            }
        }
    }

    let test_file_dir = tmp_dir_path.join("info/tests");
    fs::create_dir_all(&test_file_dir)?;

    let test_file = test_file_dir.join("tests.yaml");
    fs::write(&test_file, serde_yaml::to_string(&tests)?)?;
    test_files.push(test_file);

    Ok(test_files)
}
