use std::path::{Path, PathBuf};

use fs_err as fs;

use crate::{metadata::Output, packaging::PackagingError, recipe::parser::GlobVec as OldGlobVec};
use rattler_build_recipe::stage1::{TestType, tests::CommandsTest};

/// Convert Stage1 GlobVec to old parser GlobVec
/// Stage1 GlobVec is simpler (just a list of globs) vs old parser's include/exclude
fn convert_globvec(stage1_glob: &rattler_build_recipe::stage1::GlobVec) -> OldGlobVec {
    let strings = stage1_glob.to_strings();
    // Old parser GlobVec treats these as include globs
    let str_refs: Vec<&str> = strings.iter().map(|s| s.as_str()).collect();
    OldGlobVec::from_vec(str_refs, None)
}

fn write_test_files_to_folder(
    command_test: &CommandsTest,
    folder: &Path,
    output: &Output,
) -> Result<Vec<PathBuf>, PackagingError> {
    let mut test_files = Vec::new();

    if !command_test.files.recipe.is_empty() || !command_test.files.source.is_empty() {
        fs::create_dir_all(folder)?;
    }

    if !command_test.files.recipe.is_empty() {
        let globs = convert_globvec(&command_test.files.recipe);
        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.recipe_dir,
            folder,
        )
        .with_globvec(&globs)
        .use_gitignore(false)
        .run()?;

        test_files.extend(copy_dir.copied_paths().iter().cloned());
    }

    if !command_test.files.source.is_empty() {
        let globs = convert_globvec(&command_test.files.source);
        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.work_dir,
            folder,
        )
        .with_globvec(&globs)
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
    let tests = output.recipe.tests.clone();

    // remove the package contents tests as they are not needed in the final package
    let tests: Vec<_> = tests
        .into_iter()
        .filter(|test| !matches!(test, TestType::PackageContents { .. }))
        .collect();

    // For each `Commands` test, we need to copy the test files to the package
    for (idx, test) in tests.iter().enumerate() {
        if let TestType::Commands(command_test) = test {
            let folder = tmp_dir_path.join(format!("etc/conda/test-files/{name}/{idx}"));
            let files = write_test_files_to_folder(command_test, &folder, output)?;
            if !files.is_empty() {
                test_files.extend(files);
            }

            // TODO: In Stage1, scripts are already resolved to Vec<String> during Stage0->Stage1 transition.
            // The old parser had a ScriptContent type with cwd and content fields that needed resolution.
            // Stage1 scripts are simpler and don't need runtime resolution. If we need to store a cwd
            // for the test, we'll need to modify the Stage1 CommandsTest structure to include an
            // optional cwd field, or handle this differently in the test execution phase.
            //
            // For now, the test files are copied correctly, and the script commands are already
            // in the command_test.script Vec<String>. The tests.yaml will be serialized with
            // the resolved commands.
        }
    }

    let test_file_dir = tmp_dir_path.join("info/tests");
    fs::create_dir_all(&test_file_dir)?;

    let test_file = test_file_dir.join("tests.yaml");
    fs::write(&test_file, serde_yaml::to_string(&tests)?)?;
    test_files.push(test_file);

    Ok(test_files)
}
