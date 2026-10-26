use std::path::{Path, PathBuf};

use fs_err as fs;
use jiff::Timestamp;
use rattler_conda_types::Subdir;
use serde::{Deserialize, Serialize};

use dunce::canonicalize;

use crate::utils::{is_pending_removal, remove_dir_all_force};

/// Builder for creating [`Directories`] with a fluent API.
#[derive(Debug, Clone)]
pub struct DirectoriesBuilder<'a> {
    name: &'a str,
    recipe_path: &'a Path,
    output_dir: &'a Path,
    timestamp: &'a Timestamp,
    platform: Subdir,
    no_build_id: bool,
    variant_hash: Option<&'a str>,
    merge_build_and_host: bool,
    skip_directory_creation: bool,
}

impl<'a> DirectoriesBuilder<'a> {
    /// Create a new builder with required parameters.
    pub fn new(
        name: &'a str,
        recipe_path: &'a Path,
        output_dir: &'a Path,
        timestamp: &'a Timestamp,
        platform: Subdir,
    ) -> Self {
        Self {
            name,
            recipe_path,
            output_dir,
            timestamp,
            platform,
            no_build_id: false,
            variant_hash: None,
            merge_build_and_host: false,
            skip_directory_creation: false,
        }
    }

    /// When true, omit the build ID (timestamp) from the build directory name.
    pub fn no_build_id(mut self, no_build_id: bool) -> Self {
        self.no_build_id = no_build_id;
        self
    }

    /// The variant hash to add to the build directory name next to the build
    /// ID, so that variants of a package built at the same time (see
    /// `--parallel-builds`) do not share a build directory. Omitted when it
    /// would exceed the padded host-prefix budget, or with
    /// [`Self::no_build_id`], which keeps build directory paths short.
    pub fn variant_hash(mut self, variant_hash: Option<&'a str>) -> Self {
        self.variant_hash = variant_hash;
        self
    }

    /// When true, use the same prefix for both build and host environments.
    pub fn merge_build_and_host(mut self, merge: bool) -> Self {
        self.merge_build_and_host = merge;
        self
    }

    /// Skip creating directories on the filesystem.
    /// Useful for render-only mode where no output files are produced.
    pub fn skip_directory_creation(mut self, skip: bool) -> Self {
        self.skip_directory_creation = skip;
        self
    }

    /// Build the [`Directories`] struct.
    pub fn build(self) -> Result<Directories, std::io::Error> {
        Directories::setup_internal(self)
    }
}

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

/// The build tree as the executing build script sees it. Identical to the
/// physical layout when the script executes directly on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionDirectories {
    /// Host dependency prefix as observed by the build script.
    pub host_prefix: PathBuf,
    /// Build dependency prefix as observed by the build script.
    pub build_prefix: PathBuf,
    /// Working directory as observed by the build script.
    pub work_dir: PathBuf,
    /// Build directory as observed by the build script.
    pub build_dir: PathBuf,
    /// Recipe directory as observed by the build script.
    pub recipe_dir: PathBuf,
}

impl ExecutionDirectories {
    /// Path to the file a build script may use to override packaged files.
    pub fn package_files_list_path(&self) -> PathBuf {
        self.build_dir.join(crate::consts::PACKAGE_FILES_LIST_NAME)
    }
}

const HOST_PREFIX_LENGTH: usize = 255;

/// Host prefix directory under `build_dir` for the given platform. Windows
/// uses the short `h_env`; other platforms pad `host_env` with `_placehold`
/// repetitions so the absolute prefix path is 255 characters long.
pub fn padded_host_prefix(build_dir: &Path, platform: Subdir) -> PathBuf {
    if platform.is_windows() {
        build_dir.join("h_env")
    } else {
        let placeholder_template = "_placehold";
        let mut placeholder = String::new();

        while placeholder.len() < HOST_PREFIX_LENGTH {
            placeholder.push_str(placeholder_template);
        }

        let placeholder = placeholder
            [0..HOST_PREFIX_LENGTH - build_dir.join("host_env").as_os_str().len()]
            .to_string();

        build_dir.join(format!("host_env{placeholder}"))
    }
}

fn get_build_dir(
    output_dir: &Path,
    name: &str,
    no_build_id: bool,
    variant_hash: Option<&str>,
    timestamp: &Timestamp,
) -> Result<PathBuf, std::io::Error> {
    let since_the_epoch = timestamp.as_second();

    let dirname = match (no_build_id, variant_hash) {
        (true, _) => format!("rattler-build_{}", name),
        (false, None) => format!("rattler-build_{}_{:?}", name, since_the_epoch),
        (false, Some(hash)) => format!("rattler-build_{}_{}_{:?}", name, hash, since_the_epoch),
    };
    Ok(output_dir.join("bld").join(dirname))
}

impl Directories {
    /// Create a new [`DirectoriesBuilder`] with the required parameters.
    pub fn builder<'a>(
        name: &'a str,
        recipe_path: &'a Path,
        output_dir: &'a Path,
        timestamp: &'a Timestamp,
        platform: Subdir,
    ) -> DirectoriesBuilder<'a> {
        DirectoriesBuilder::new(name, recipe_path, output_dir, timestamp, platform)
    }

    /// Internal setup function called by the builder.
    fn setup_internal(builder: DirectoriesBuilder<'_>) -> Result<Directories, std::io::Error> {
        let DirectoriesBuilder {
            name,
            recipe_path,
            output_dir,
            timestamp,
            platform,
            no_build_id,
            variant_hash,
            merge_build_and_host,
            skip_directory_creation,
        } = builder;

        let output_dir = if skip_directory_creation {
            output_dir.to_path_buf()
        } else {
            if !output_dir.exists() {
                fs::create_dir_all(output_dir)?;
            }

            // Write .condapackageignore to exclude the output directory from source copying.
            // This prevents the output directory from being included when users use `path: ../`
            // in their source configuration.
            let ignore_file = output_dir.join(".condapackageignore");
            if !ignore_file.exists() {
                fs::write(&ignore_file, "*\n")?;
            }

            canonicalize(output_dir)?
        };

        let build_dir = get_build_dir(&output_dir, name, no_build_id, variant_hash, timestamp)
            .expect("Could not create build directory");
        let build_dir = if !platform.is_windows()
            && !no_build_id
            && variant_hash.is_some()
            && build_dir.as_os_str().len() + "/host_env".len() > HOST_PREFIX_LENGTH
        {
            // The scheduler serializes outputs sharing a directory when the hash cannot fit.
            get_build_dir(&output_dir, name, no_build_id, None, timestamp)?
        } else {
            build_dir
        };
        // TODO move this into build_dir, and keep build_dir consistent.
        let cache_dir = output_dir.join("build_cache");
        let recipe_dir = recipe_path
            .parent()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "Parent directory not found")
            })?
            .to_path_buf();

        let host_prefix = padded_host_prefix(&build_dir, platform);

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

        if !skip_directory_creation {
            directories.log_build_folder()?;
        }

        Ok(directories)
    }

    /// Path to the file pointed at by the `RATTLER_BUILD_PACKAGE_FILES`
    /// environment variable. Build scripts may write paths to this file (one
    /// per line) to override the default mechanism that determines which files
    /// end up in the final package.
    pub fn package_files_list_path(&self) -> PathBuf {
        self.build_dir.join(crate::consts::PACKAGE_FILES_LIST_NAME)
    }

    /// The path strings the build script observes.
    pub fn exec_view(&self) -> ExecutionDirectories {
        ExecutionDirectories {
            host_prefix: self.host_prefix.clone(),
            build_prefix: self.build_prefix.clone(),
            work_dir: self.work_dir.clone(),
            build_dir: self.build_dir.clone(),
            recipe_dir: self.recipe_dir.clone(),
        }
    }

    /// Remove all directories except for the cache directory
    pub fn clean(&self) -> Result<(), std::io::Error> {
        if self.build_dir.exists() {
            // Snapshot entries before iterating: on Windows the rename-before-
            // delete path in `remove_dir_all_force` creates new sibling trash
            // dirs (`.{name}.pending-rm-{nanos}`), and we don't want the
            // iterator to pick them up and process them recursively.
            let folders: Vec<_> = self.build_dir.read_dir()?.collect::<Result<_, _>>()?;
            for folder in folders {
                let path = folder.path();

                if path == self.cache_dir {
                    continue;
                }

                // Leave pending-rm trash dirs (from this or a previous run)
                // alone. Re-cleaning them stacks `.pending-rm-*` suffixes and
                // can blow past Windows' MAX_PATH, and the underlying files
                // are still locked by whatever blocked removal originally.
                if is_pending_removal(&path) {
                    continue;
                }

                if folder.file_type()?.is_dir() {
                    remove_dir_all_force(&path)?;
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
        let p1 = get_build_dir(dir.path(), "name", true, None, &Timestamp::now()).unwrap();
        let f1 = p1.file_name().unwrap();
        assert!(f1.eq("rattler-build_name"));

        // without build_id, the variant hash is not used either (short paths)
        let p1 = get_build_dir(
            dir.path(),
            "name",
            true,
            Some("h1234567"),
            &Timestamp::now(),
        )
        .unwrap();
        assert!(p1.file_name().unwrap().eq("rattler-build_name"));

        // with build_id (aka timestamp)
        let timestamp = &Timestamp::now();
        let p2 = get_build_dir(dir.path(), "name", false, None, timestamp).unwrap();
        let f2 = p2.file_name().unwrap();
        let epoch = timestamp.as_second();
        assert!(f2.eq(format!("rattler-build_name_{epoch}").as_str()));

        // with build_id and a variant hash: variants built in the same second
        // get different build directories
        let p3 = get_build_dir(dir.path(), "name", false, Some("h1234567"), timestamp).unwrap();
        let p4 = get_build_dir(dir.path(), "name", false, Some("h7654321"), timestamp).unwrap();
        assert!(
            p3.file_name()
                .unwrap()
                .eq(format!("rattler-build_name_h1234567_{epoch}").as_str())
        );
        assert_ne!(p3, p4);
    }

    #[test]
    fn variant_directories_respect_host_prefix_budget() {
        let tempdir = tempfile::tempdir().unwrap();
        let timestamp = Timestamp::now();
        let base_length = get_build_dir(tempdir.path(), "name", false, None, &timestamp)
            .unwrap()
            .join("host_env")
            .as_os_str()
            .len();

        for (unpadded_length, distinct) in [(246, true), (247, false), (255, false)] {
            let output_dir = tempdir
                .path()
                .join("a".repeat(unpadded_length - base_length - 1));
            let recipe_path = tempdir.path().join("recipe.yaml");
            let directories = |hash| {
                Directories::builder(
                    "name",
                    &recipe_path,
                    &output_dir,
                    &timestamp,
                    Subdir::Linux64,
                )
                .variant_hash(Some(hash))
                .skip_directory_creation(true)
                .build()
                .unwrap()
            };
            let first = directories("h1234567");
            let second = directories("h7654321");
            assert_eq!(first.host_prefix.as_os_str().len(), HOST_PREFIX_LENGTH);
            assert_eq!(second.host_prefix.as_os_str().len(), HOST_PREFIX_LENGTH);
            if distinct {
                assert_ne!(first.build_dir, second.build_dir);
            } else {
                assert_eq!(first.build_dir, second.build_dir);
            }
        }
    }

    #[test]
    fn padded_host_prefix_follows_the_platform() {
        let tempdir = tempfile::tempdir().unwrap();
        let build_dir = tempdir.path().join("build");

        for platform in [Subdir::Linux64, Subdir::OsxArm64] {
            assert_eq!(
                padded_host_prefix(&build_dir, platform).as_os_str().len(),
                255
            );
        }
        assert_eq!(
            padded_host_prefix(&build_dir, Subdir::Win64),
            build_dir.join("h_env")
        );
    }

    #[test]
    fn current_platform_padding_matches_built_directories() {
        let tempdir = tempfile::tempdir().unwrap();
        let directories = Directories::builder(
            "name",
            &tempdir.path().join("recipe"),
            &tempdir.path().join("output"),
            &Timestamp::now(),
            Subdir::current().unwrap(),
        )
        .build()
        .unwrap();

        assert_eq!(
            directories.host_prefix,
            padded_host_prefix(&directories.build_dir, Subdir::current().unwrap())
        );

        let execution = directories.exec_view();
        assert_eq!(execution.host_prefix, directories.host_prefix);
        assert_eq!(execution.build_prefix, directories.build_prefix);
        assert_eq!(execution.work_dir, directories.work_dir);
        assert_eq!(execution.build_dir, directories.build_dir);
        assert_eq!(execution.recipe_dir, directories.recipe_dir);
        assert_eq!(
            execution.package_files_list_path(),
            directories.package_files_list_path()
        );
    }

    #[test]
    fn test_clean_skips_pending_rm_dirs() {
        let tempdir = tempfile::tempdir().unwrap();

        let directories = Directories::builder(
            "name",
            &tempdir.path().join("recipe"),
            &tempdir.path().join("output"),
            &Timestamp::now(),
            Subdir::current().unwrap(),
        )
        .build()
        .unwrap();
        directories.recreate_directories().unwrap();

        // Simulate a leftover trash dir from a previous rename-before-delete
        // attempt. `clean()` must leave it alone — attempting to remove it
        // would stack another `.pending-rm-*` suffix on Windows and waste
        // retries on files the OS still holds open.
        let trash = directories
            .build_dir
            .join(".work.pending-rm-1776529982099702900");
        fs::create_dir_all(&trash).unwrap();
        fs::write(trash.join("locked.txt"), b"content").unwrap();

        directories.clean().unwrap();

        assert!(trash.exists(), "pending-rm trash dir must be preserved");
        assert!(
            !directories.work_dir.exists(),
            "regular work dir should still be cleaned"
        );
    }

    #[test]
    fn test_directories_yaml_rendering() {
        let tempdir = tempfile::tempdir().unwrap();

        let directories = Directories::builder(
            "name",
            &tempdir.path().join("recipe"),
            &tempdir.path().join("output"),
            &Timestamp::now(),
            Subdir::current().unwrap(),
        )
        .build()
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
