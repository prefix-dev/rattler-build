use std::path::{Path, PathBuf};

use fs_err as fs;

use crate::{
    metadata::Output,
    packaging::PackagingError,
    recipe::parser::{CommandsTest, TestType},
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
            .use_gitignore(true)
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
            .use_gitignore(true)
            .run()?;

            test_files.extend(copy_dir.copied_paths().iter().cloned());
        }

        Ok(test_files)
    }
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

    // For each `Command` test, we need to copy the test files to the package
    for (idx, test) in tests.iter_mut().enumerate() {
        if let TestType::Command(ref mut command_test) = test {
            let cwd = PathBuf::from(format!("etc/conda/test-files/{name}/{idx}"));
            let folder = tmp_dir_path.join(&cwd);
            let files = command_test.write_to_folder(&folder, output)?;
            if !files.is_empty() {
                test_files.extend(files);
                // store the cwd in the rendered test
                command_test.script.cwd = Some(cwd);
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
