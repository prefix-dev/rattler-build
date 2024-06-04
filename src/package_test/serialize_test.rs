use fs_err as fs;
use fs_err::File;
use rattler_conda_types::Platform;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use crate::{
    metadata::Output,
    packaging::PackagingError,
    recipe::parser::{CommandsTest, DownstreamTest, PythonTest, TestType},
};

impl DownstreamTest {
    fn write_to_folder(&self, folder: &Path) -> Result<Vec<PathBuf>, PackagingError> {
        fs::create_dir_all(folder)?;
        let path = folder.join("downstream_test.json");
        let mut file = File::create(&path)?;
        file.write_all(serde_json::to_string(self)?.as_bytes())?;
        Ok(vec![path])
    }
}

impl CommandsTest {
    fn write_to_folder(
        &self,
        folder: &Path,
        output: &Output,
    ) -> Result<Vec<PathBuf>, PackagingError> {
        let mut command_files = vec![];
        let mut test_files = vec![];

        fs::create_dir_all(folder)?;

        let target_platform = &output.build_configuration.target_platform;
        if target_platform.is_windows() || target_platform == &Platform::NoArch {
            command_files.push(folder.join("run_test.bat"));
        }

        if target_platform.is_unix() || target_platform == &Platform::NoArch {
            command_files.push(folder.join("run_test.sh"));
        }

        for cf in command_files {
            let mut file = File::create(&cf)?;
            for el in &self.script {
                writeln!(file, "{}\n", el)?;
            }
            test_files.push(cf);
        }

        if !self.requirements.is_empty() {
            let test_dependencies = &self.requirements;
            let test_file = folder.join("test_time_dependencies.json");
            let mut file = File::create(&test_file)?;
            file.write_all(serde_json::to_string(&test_dependencies)?.as_bytes())?;
            test_files.push(test_file);
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

impl PythonTest {
    fn write_to_folder(&self, folder: &Path) -> Result<Vec<PathBuf>, PackagingError> {
        fs::create_dir_all(folder)?;
        let path = folder.join("python_test.json");
        serde_json::to_writer(&File::create(&path)?, self)?;
        Ok(vec![path])
    }
}

/// Write out the test files for the final package
pub(crate) fn write_test_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let mut test_files = Vec::new();

    for (idx, test) in output.recipe.tests().iter().enumerate() {
        let folder = tmp_dir_path.join(format!("info/tests/{}", idx));
        let files = match test {
            TestType::Python(python_test) => python_test.write_to_folder(&folder)?,
            TestType::Command(command_test) => command_test.write_to_folder(&folder, output)?,
            TestType::Downstream(downstream_test) => downstream_test.write_to_folder(&folder)?,
            TestType::PackageContents(_) => Vec::new(),
        };
        test_files.extend(files);
    }

    Ok(test_files)
}
