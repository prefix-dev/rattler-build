use std::path::{Path, PathBuf};

use fs_err as fs;

use crate::{
    metadata::Output,
    packaging::PackagingError,
    recipe::{
        Jinja,
        parser::{CommandsTest, ScriptContent, TestType},
    },
    script::ResolvedScriptContents,
};

impl CommandsTest {
    fn write_to_folder(
        &self,
        folder: &Path,
        output: &Output,
    ) -> Result<Vec<PathBuf>, PackagingError> {
        let mut test_files = Vec::new();

        if !self.files.recipe.is_empty() || !self.files.source.is_empty() {
            fs::create_dir_all(folder)?;
        }

        if !self.files.recipe.is_empty() {
            let globs = &self.files.recipe;
            let copy_dir = crate::source::copy_dir::CopyDir::new(
                &output.build_configuration.directories.recipe_dir,
                folder,
            )
            .with_globvec(globs)
            .use_gitignore(false)
            .run()?;

            test_files.extend(copy_dir.copied_paths().iter().cloned());
        }

        if !self.files.source.is_empty() {
            let globs = &self.files.source;
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
}

fn default_jinja_context(output: &Output) -> Jinja {
    let selector_config = output.build_configuration.selector_config();
    Jinja::new(selector_config).with_context(&output.recipe.context)
}

/// Write out the test files for the final package
pub(crate) fn write_test_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let mut test_files = Vec::new();

    let name = output.name().as_normalized();

    // extract test section from the original recipe
    let mut tests = output.recipe.tests().clone();

    // remove the package contents tests as they are not needed in the final package
    tests.retain(|test| !matches!(test, TestType::PackageContents { .. }));

    // For each `Command` test, we need to copy the test files to the package
    for (idx, test) in tests.iter_mut().enumerate() {
        if let TestType::Command(command_test) = test {
            let cwd = PathBuf::from(format!("etc/conda/test-files/{name}/{idx}"));
            let folder = tmp_dir_path.join(&cwd);
            let files = command_test.write_to_folder(&folder, output)?;
            if !files.is_empty() {
                test_files.extend(files);
                // store the cwd in the rendered test
                command_test.script.cwd = Some(cwd);
            }

            // Try to render the script contents here
            // Note: we want to improve this with better rendering in the future
            let contents = command_test.script.resolve_content(
                &output.build_configuration.directories.recipe_dir,
                Some(default_jinja_context(output)),
                &["sh", "bat"],
            )?;

            // Replace with rendered contents
            match contents {
                ResolvedScriptContents::Inline(contents) => {
                    command_test.script.content = ScriptContent::Command(contents)
                }
                ResolvedScriptContents::Path(_path, contents) => {
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
