use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use fs_err as fs;
use serde::{Deserialize, Serialize};

use dunce::canonicalize;

use crate::utils::remove_dir_all_force;

/// Directories used during the build process
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Directories {
    /// The directory where the recipe is located
    #[serde(skip)]
    pub recipe_dir: PathBuf,
    /// The path where the recipe is located
    #[serde(skip)]
    pub recipe_path: PathBuf,
    /// The folder where the cache is located
    #[serde(skip)]
    pub cache_dir: PathBuf,
    /// The host prefix is the directory where host dependencies are installed
    /// Exposed as `$PREFIX` (or `%PREFIX%` on Windows) in the build script
    pub host_prefix: PathBuf,
    /// The build prefix is the directory where build dependencies are installed
    /// Exposed as `$BUILD_PREFIX` (or `%BUILD_PREFIX%` on Windows) in the build
    /// script
    pub build_prefix: PathBuf,
    /// The work directory is the directory where the source code is copied to
    pub work_dir: PathBuf,
    /// The parent directory of host, build and work directories
    pub build_dir: PathBuf,
    /// The output directory or local channel directory
    #[serde(skip)]
    pub output_dir: PathBuf,
}

fn get_build_dir(
    output_dir: &Path,
    name: &str,
    no_build_id: bool,
    timestamp: &DateTime<Utc>,
) -> Result<PathBuf, std::io::Error> {
    let since_the_epoch = timestamp.timestamp();

    let dirname = if no_build_id {
        format!("rattler-build_{}", name)
    } else {
        format!("rattler-build_{}_{:?}", name, since_the_epoch)
    };
    Ok(output_dir.join("bld").join(dirname))
}

impl Directories {
    /// Create all directories needed for the building of a package
    pub fn setup(
        name: &str,
        recipe_path: &Path,
        output_dir: &Path,
        no_build_id: bool,
        timestamp: &DateTime<Utc>,
        merge_build_and_host: bool,
    ) -> Result<Directories, std::io::Error> {
        if !output_dir.exists() {
            fs::create_dir_all(output_dir)?;
        }
        let output_dir = canonicalize(output_dir)?;

        // Write .condapackageignore to exclude the output directory from source copying.
        // This prevents the output directory from being included when users use `path: ../`
        // in their source configuration.
        let ignore_file = output_dir.join(".condapackageignore");
        if !ignore_file.exists() {
            fs::write(&ignore_file, "*\n")?;
        }

        let build_dir = get_build_dir(&output_dir, name, no_build_id, timestamp)
            .expect("Could not create build directory");
        // TODO move this into build_dir, and keep build_dir consistent.
        let cache_dir = output_dir.join("build_cache");
        let recipe_dir = recipe_path
            .parent()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "Parent directory not found")
            })?
            .to_path_buf();

        let host_prefix = if cfg!(target_os = "windows") {
            build_dir.join("h_env")
        } else {
            let placeholder_template = "_placehold";
            let mut placeholder = String::new();
            let placeholder_length: usize = 255;

            while placeholder.len() < placeholder_length {
                placeholder.push_str(placeholder_template);
            }

            let placeholder = placeholder
                [0..placeholder_length - build_dir.join("host_env").as_os_str().len()]
                .to_string();

            build_dir.join(format!("host_env{}", placeholder))
        };

        let directories = Directories {
            build_dir: build_dir.clone(),
            build_prefix: if merge_build_and_host {
                host_prefix.clone()
            } else {
                build_dir.join("build_env")
            },
            cache_dir,
            host_prefix,
            work_dir: build_dir.join("work"),
            recipe_dir,
            recipe_path: recipe_path.to_path_buf(),
            output_dir,
        };

        // Log the build folder for debugging
        directories.log_build_folder()?;

        Ok(directories)
    }

    /// Remove all directories except for the cache directory
    pub fn clean(&self) -> Result<(), std::io::Error> {
        if self.build_dir.exists() {
            let folders = self.build_dir.read_dir()?;
            for folder in folders {
                let folder = folder?;

                if folder.path() == self.cache_dir {
                    continue;
                }

                if folder.file_type()?.is_dir() {
                    remove_dir_all_force(&folder.path())?;
                }
            }
        }
        Ok(())
    }

    /// Log the build folder to rattler-build-log.txt for debugging purposes
    pub fn log_build_folder(&self) -> Result<(), std::io::Error> {
        let log_file = self.output_dir.join("rattler-build-log.txt");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)?;

        use std::io::Write;

        // Create a JSON object with all directory information
        let log_entry = serde_json::json!({
            "work_dir": self.work_dir,
            "build_dir": self.build_dir,
            "host_prefix": self.host_prefix,
            "build_prefix": self.build_prefix,
            "recipe_dir": self.recipe_dir,
            "recipe_path": self.recipe_path,
            "output_dir": self.output_dir,
            "cache_dir": self.cache_dir,
        });

        // Write as a single JSON line
        writeln!(file, "{}", serde_json::to_string(&log_entry)?)?;
        Ok(())
    }

    /// Creates the build directory.
    pub fn create_build_dir(&self, remove_existing_work_dir: bool) -> Result<(), std::io::Error> {
        if remove_existing_work_dir && self.work_dir.exists() {
            fs::remove_dir_all(&self.work_dir)?;
        }

        fs::create_dir_all(&self.work_dir)?;

        Ok(())
    }

    /// create all directories
    pub fn recreate_directories(&self) -> Result<(), std::io::Error> {
        if self.build_dir.exists() {
            fs::remove_dir_all(&self.build_dir)?;
        }

        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)?;
        }

        // Write .condapackageignore to exclude the output directory from source copying.
        // This prevents the output directory from being included when users use `path: ../`
        // in their source configuration.
        let ignore_file = self.output_dir.join(".condapackageignore");
        if !ignore_file.exists() {
            fs::write(&ignore_file, "*\n")?;
        }

        fs::create_dir_all(&self.build_dir)?;
        fs::create_dir_all(&self.work_dir)?;
        fs::create_dir_all(&self.build_prefix)?;
        fs::create_dir_all(&self.host_prefix)?;

        // Log the build folder for debugging
        self.log_build_folder()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_build_dir_test() {
        // without build_id (aka timestamp)
        let dir = tempfile::tempdir().unwrap();
        let p1 = get_build_dir(dir.path(), "name", true, &Utc::now()).unwrap();
        let f1 = p1.file_name().unwrap();
        assert!(f1.eq("rattler-build_name"));

        // with build_id (aka timestamp)
        let timestamp = &Utc::now();
        let p2 = get_build_dir(dir.path(), "name", false, timestamp).unwrap();
        let f2 = p2.file_name().unwrap();
        let epoch = timestamp.timestamp();
        assert!(f2.eq(format!("rattler-build_name_{epoch}").as_str()));
    }

    #[test]
    fn test_directories_yaml_rendering() {
        let tempdir = tempfile::tempdir().unwrap();

        let directories = Directories::setup(
            "name",
            &tempdir.path().join("recipe"),
            &tempdir.path().join("output"),
            false,
            &chrono::Utc::now(),
            false,
        )
        .unwrap();
        directories.create_build_dir(false).unwrap();

        // test yaml roundtrip
        let yaml = serde_yaml::to_string(&directories).unwrap();
        let directories2: Directories = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(directories.build_dir, directories2.build_dir);
        assert_eq!(directories.build_prefix, directories2.build_prefix);
        assert_eq!(directories.host_prefix, directories2.host_prefix);
    }
}
