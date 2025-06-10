//! Functions for applying patches to a work directory.
use super::SourceError;
use crate::system_tools::{SystemTools, Tool};
use itertools::Itertools;
use std::io::Write;
use std::process::{Command, Output};
use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
};

use diffy::{Patch, patches_from_str_with_config};
use fs_err::File;
use walkdir::WalkDir;

fn parse_patches(patches: &Vec<Patch<str>>) -> HashSet<PathBuf> {
    let mut affected_files = HashSet::new();

    for patch in patches {
        if let Some(p) = patch
            .original()
            .filter(|p| p.trim() != "/dev/null")
            .map(PathBuf::from)
        {
            affected_files.insert(p);
        }
        if let Some(p) = patch
            .modified()
            .filter(|p| p.trim() != "/dev/null")
            .map(PathBuf::from)
        {
            affected_files.insert(p);
        }
    }

    affected_files
}

fn patches_from_str(input: &str) -> Result<Vec<Patch<'_, str>>, diffy::ParsePatchError> {
    patches_from_str_with_config(
        input,
        diffy::ParserConfig {
            hunk_strategy: diffy::HunkRangeStrategy::Recount,
        },
    )
}

fn apply(base_image: &str, patch: &Patch<'_, str>) -> Result<String, diffy::ApplyError> {
    diffy::apply_with_config(
        base_image,
        patch,
        &diffy::FuzzyConfig {
            max_fuzz: 2,
            ignore_whitespace: true,
            ignore_case: false,
        },
    )
}

// XXX: This could become a bottleneck as it is called at least twice
// for the same directory, but currently not major performance
// regressions observed.
/// Try to find path as a subdirectory path of some directory. Returns
/// shortest matching path if one exists.
fn find_as_subdir(p: &Path, sub: &Path) -> Option<PathBuf> {
    WalkDir::new(p)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().ends_with(sub))
        .map(|e| e.path().to_owned())
        .min_by(|a, b| a.components().count().cmp(&b.components().count()))
}

// Returns number by which all patch paths must be stripped to be
// successfully applied, or returns and error if no such number could
// be determined.
fn guess_strip_level(patch: &Vec<Patch<str>>, work_dir: &Path) -> Result<usize, SourceError> {
    // Assume that no /dev/null in here
    let patched_files = parse_patches(patch);

    let max_components = patched_files
        .iter()
        .map(|p| p.components().count())
        .max()
        .unwrap_or(0);

    for strip_level in 0..max_components {
        let all_paths_exist = patched_files
            .iter()
            .map(|p| {
                let path: PathBuf = p.components().skip(strip_level).collect();
                find_as_subdir(work_dir, &path)
            })
            .all(|p| p.map(|p| p.exists()).unwrap_or(false));
        if all_paths_exist {
            return Ok(strip_level);
        }
    }

    // XXX: This is not entirely correct way of handling this, since
    // path is not necessarily starts with meaningless one letter
    // component. Proper handling requires more in-depth analysis.
    // For example this is fine if source is /dev/null and target is
    // not, but may be incorrect otherwise, if original file does not
    // exist.
    Ok(1)

    // Err(SourceError::PatchFailed(String::from(
    //     "can't find files to be patched",
    // )))
}

fn custom_patch_stripped_paths(
    patch: &Patch<'_, str>,
    strip_level: usize,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let original = (patch.original(), patch.modified());
    let stripped = (
        // XXX: Probably absolute paths should be checked as well. But
        // it is highly unlikely to meet them in patches, so we ignore
        // that for now.
        original.0.and_then(|p| {
            (p.trim() != "/dev/null").then(|| {
                PathBuf::from(p)
                    .components()
                    .skip(strip_level)
                    .collect::<PathBuf>()
            })
        }),
        original.1.and_then(|p| {
            (p.trim() != "/dev/null").then(|| {
                PathBuf::from(p)
                    .components()
                    .skip(strip_level)
                    .collect::<PathBuf>()
            })
        }),
    );
    stripped
}

fn write_patch_content(content: &str, path: &Path) -> Result<(), SourceError> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent).map_err(SourceError::Io)?;
    }

    let mut new_file = File::create(path).map_err(SourceError::Io)?;
    new_file
        .write_all(content.as_bytes())
        .map_err(SourceError::Io)?;

    Ok(())
}

#[allow(dead_code)]
pub(crate) fn apply_patch_custom(
    work_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), SourceError> {
    let patch_file_content = fs_err::read_to_string(patch_file_path).map_err(SourceError::Io)?;

    let patches = patches_from_str(&patch_file_content)
        .map_err(|_| SourceError::PatchParseFailed(patch_file_path.to_path_buf()))?;
    let strip_level = guess_strip_level(&patches, work_dir)?;

    for patch in patches {
        let file_paths = custom_patch_stripped_paths(&patch, strip_level);
        let absolute_file_paths = (
            file_paths.0.and_then(|o| find_as_subdir(work_dir, &o)),
            file_paths.1.and_then(|m| find_as_subdir(work_dir, &m)),
        );

        tracing::debug!(
            "Patch will be applied:\n\tFrom: {:#?}\n\tTo:{:#?}",
            absolute_file_paths.0,
            absolute_file_paths.1
        );

        match absolute_file_paths {
            (None, None) => continue,
            (None, Some(m)) => {
                let new_file_content = apply("", &patch).map_err(SourceError::PatchApplyError)?;
                write_patch_content(&new_file_content, &m)?;
            }
            (Some(o), None) => {
                fs_err::remove_file(work_dir.join(o)).map_err(SourceError::Io)?;
            }
            (Some(o), Some(m)) => {
                let old_file_content = fs_err::read_to_string(&o).map_err(SourceError::Io)?;

                let new_file_content =
                    apply(&old_file_content, &patch).map_err(SourceError::PatchApplyError)?;

                if o != m {
                    fs_err::remove_file(&o).map_err(SourceError::Io)?;
                }

                write_patch_content(&new_file_content, &m)?;
            }
        }
    }

    Ok(())
}

pub(crate) fn apply_patch_git(
    system_tools: &SystemTools,
    work_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), SourceError> {
    let patch_file_content = fs_err::read_to_string(patch_file_path).map_err(SourceError::Io)?;
    let patches = patches_from_str(&patch_file_content)
        .map_err(|_| SourceError::PatchParseFailed(patch_file_path.to_path_buf()))?;
    let strip_level = guess_strip_level(&patches, work_dir)?;

    struct GitApplyAttempt {
        command: Command,
        output: Output,
    }

    let mut outputs = Vec::new();
    for try_extra_flag in [None, Some("--recount")] {
        let mut cmd_builder = system_tools
            .call(Tool::Git)
            .map_err(SourceError::GitNotFound)?;
        cmd_builder
            .current_dir(work_dir)
            .arg("apply")
            .arg(format!("-p{}", strip_level))
            .arg("--verbose")
            .arg("--ignore-space-change")
            .arg("--ignore-whitespace")
            .args(try_extra_flag.into_iter())
            .arg(patch_file_path.as_os_str())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        tracing::debug!(
            "Running: {} {}",
            cmd_builder.get_program().to_string_lossy(),
            cmd_builder
                .get_args()
                .map(OsStr::to_string_lossy)
                .format(" ")
        );

        let output = cmd_builder.output().map_err(SourceError::Io)?;
        outputs.push(GitApplyAttempt {
            command: cmd_builder,
            output: output.clone(),
        });

        if outputs
            .last()
            .expect("we just added an entry")
            .output
            .status
            .success()
        {
            break;
        }
    }

    // Check if the last output was successful, if not, we report all the errors.
    let last_output = outputs.last().expect("we just added at least one entry");
    if !last_output.output.status.success() {
        return Err(SourceError::PatchFailed(format!(
            "{}\n`git apply` failed with a combination of flags.\n\n{}",
            patch_file_path.display(),
            outputs
                .into_iter()
                .map(
                    |GitApplyAttempt {
                         output, command, ..
                     }| {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        format!(
                            "With the che command:\n\n\t{} {}The output was:\n\n\t{}\n\n",
                            command.get_program().to_string_lossy(),
                            command.get_args().map(OsStr::to_string_lossy).format(" "),
                            stderr.lines().format("\n\t")
                        )
                    }
                )
                .format("\n\n")
        )));
    }

    // Sometimes git apply will skip the contents of a patch. This usually is *not* what we
    // want, so we detect this behavior and return an error.
    let stderr = String::from_utf8_lossy(&last_output.output.stderr);
    let skipped_patch = stderr
        .lines()
        .any(|line| line.starts_with("Skipped patch "));
    if skipped_patch {
        return Err(SourceError::PatchFailed(format!(
            "{}\n`git apply` seems to have skipped some of the contents of the patch. The output of the command is:\n\n\t{}\n\nThe command was invoked with:\n\n\t{} {}",
            patch_file_path.display(),
            stderr.lines().format("\n\t"),
            last_output.command.get_program().to_string_lossy(),
            last_output
                .command
                .get_args()
                .map(OsStr::to_string_lossy)
                .format(" ")
        )));
    }

    Ok(())
}

/// Applies all patches in a list of patches to the specified work directory
/// Currently only supports patching with the `patch` command.
pub(crate) fn apply_patches(
    patches: &[PathBuf],
    work_dir: &Path,
    recipe_dir: &Path,
    apply_patch: impl Fn(&Path, &Path) -> Result<(), SourceError>,
) -> Result<(), SourceError> {
    // Early out to avoid unnecessary work
    if patches.is_empty() {
        return Ok(());
    }

    let git_dir = work_dir.join(".git");
    // Ensure that the working directory is a valid git directory.
    let _dot_git_dir = if !git_dir.exists() {
        Some(TempDotGit::setup(work_dir)?)
    } else {
        None
    };

    for patch_path_relative in patches {
        let patch_file_path = recipe_dir.join(patch_path_relative);

        tracing::info!("Applying patch: {}", patch_file_path.to_string_lossy());

        if !patch_file_path.exists() {
            return Err(SourceError::PatchNotFound(patch_file_path));
        }

        apply_patch(work_dir, patch_file_path.as_path())?;
    }
    Ok(())
}

/// A temporary .git directory that contains the bare minimum files and
/// directories needed for git to function as if the directory that contains
/// the .git directory is a proper git repository.
struct TempDotGit {
    path: PathBuf,
}

impl TempDotGit {
    /// Creates a temporary .git directory in the specified root directory.
    fn setup(root: &Path) -> std::io::Result<Self> {
        // Initialize a temporary .git directory
        let dot_git = root.join(".git");
        fs_err::create_dir(&dot_git)?;
        let dot_git = TempDotGit { path: dot_git };

        // Add the minimum number of files and directories to the .git directory that are needed for
        // git to work
        fs_err::create_dir(dot_git.path.join("objects"))?;
        fs_err::create_dir(dot_git.path.join("refs"))?;
        fs_err::write(dot_git.path.join("HEAD"), "ref: refs/heads/main")?;

        Ok(dot_git)
    }
}

impl Drop for TempDotGit {
    fn drop(&mut self) {
        fs_err::remove_dir_all(&self.path).unwrap_or_else(|e| {
            eprintln!("Failed to remove temporary .git directory: {}", e);
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        get_build_output, get_tool_config,
        metadata::Output,
        opt::{BuildData, BuildOpts, CommonOpts},
        recipe::parser::Source,
        script::SandboxArguments,
        source::{copy_dir::CopyDir, fetch_sources},
        tool_configuration::Configuration,
    };
    use std::{ffi::OsStr, process::Command};

    use super::*;
    use line_ending::LineEnding;
    use miette::{IntoDiagnostic, ensure};
    use rstest::*;

    use tempfile::TempDir;

    #[test]
    fn test_parse_patch() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patches");

        // for all patches, just try parsing the patch
        for entry in patches_dir.read_dir().unwrap() {
            let patch = entry.unwrap();
            let patch_path = patch.path();
            if patch_path.extension() != Some("patch".as_ref()) {
                continue;
            }

            let patch_file_content =
                fs_err::read_to_string(&patch_path).expect("Could not read file contents");
            let _ = patches_from_str(&patch_file_content).expect("Failed to parse patch file");

            println!("Parsing patch: {}", patch_path.display());
        }
    }

    #[test]
    fn get_affected_files() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patch_application/patches");

        let patch_file_content = fs_err::read_to_string(patches_dir.join("test.patch"))
            .expect("Could not read file contents");
        let patches = patches_from_str(&patch_file_content).expect("Failed to parse patch file");

        let patched_paths = parse_patches(&patches);
        assert_eq!(patched_paths.len(), 2);
        assert!(patched_paths.contains(&PathBuf::from("a/text.md")));
        assert!(patched_paths.contains(&PathBuf::from("b/text.md")));

        let patch_file_content =
            fs_err::read_to_string(patches_dir.join("0001-increase-minimum-cmake-version.patch"))
                .expect("Could not read file contents");
        let patches = patches_from_str(&patch_file_content).expect("Failed to parse patch file");
        let patched_paths = parse_patches(&patches);
        assert_eq!(patched_paths.len(), 2);
        assert!(patched_paths.contains(&PathBuf::from("a/CMakeLists.txt")));
        assert!(patched_paths.contains(&PathBuf::from("b/CMakeLists.txt")));
    }

    fn setup_patch_test_dir() -> (TempDir, PathBuf) {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patch_test_dir = manifest_dir.join("test-data/patch_application");

        let tempdir = TempDir::new().unwrap();
        let _ = CopyDir::new(&patch_test_dir, tempdir.path()).run().unwrap();

        (tempdir, patch_test_dir)
    }

    #[test]
    fn test_apply_patches() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with normal patch
        apply_patches(
            &[PathBuf::from("test.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            |wd, p| apply_patch_git(&SystemTools::new(), wd, p),
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
    }

    #[test]
    fn test_apply_patches_with_crlf() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with CRLF patch
        let patch = tempdir.path().join("patches/test.patch");
        let text = fs_err::read_to_string(&patch).unwrap();
        let clrf_patch = LineEnding::CRLF.apply(&text);

        fs_err::write(tempdir.path().join("patches/test_crlf.patch"), clrf_patch).unwrap();

        // Test with CRLF patch
        apply_patches(
            &[PathBuf::from("test_crlf.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            |wd, p| apply_patch_git(&SystemTools::new(), wd, p),
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
    }

    #[test]
    fn test_apply_0001_increase_minimum_cmake_version_patch() {
        let (tempdir, _) = setup_patch_test_dir();

        apply_patches(
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            |wd, p| apply_patch_git(&SystemTools::new(), wd, p),
        )
        .expect("Patch 0001-increase-minimum-cmake-version.patch should apply successfully");

        // Read the cmake list file and make sure that it contains `cmake_minimum_required(VERSION 3.12)`
        let cmake_list = tempdir.path().join("workdir/CMakeLists.txt");
        let cmake_list = fs_err::read_to_string(&cmake_list).unwrap();
        assert!(cmake_list.contains("cmake_minimum_required(VERSION 3.12)"));
    }

    #[test]
    fn test_apply_git_patch_in_git_ignored() {
        let (tempdir, _) = setup_patch_test_dir();

        // Initialize a temporary .git directory at the root of the temporary directory. This makes
        // git take the working directory is in a git repository.
        let _temp_dot_git = TempDotGit::setup(tempdir.path()).unwrap();

        // Apply the patches in the working directory
        apply_patches(
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            |wd, p| apply_patch_git(&SystemTools::new(), wd, p),
        )
        .expect("Patch 0001-increase-minimum-cmake-version.patch should apply successfully");

        // Read the cmake list file and make sure that it contains `cmake_minimum_required(VERSION 3.12)`
        let cmake_list = tempdir.path().join("workdir/CMakeLists.txt");
        let cmake_list = fs_err::read_to_string(&cmake_list).unwrap();
        assert!(cmake_list.contains("cmake_minimum_required(VERSION 3.12)"));
    }

    type PatchableSource = (BuildData, Configuration, Output, Vec<Source>);
    type PatchablePkg = (TempDir, Vec<PatchableSource>);

    /// Prepare all information needed to test patches for package info path.
    async fn prepare_package(recipe_dir: &Path) -> miette::Result<PatchablePkg> {
        let artifacts_dir = tempfile::tempdir().unwrap();
        let artifacts_dir_path = artifacts_dir.path();
        let recipe_path = recipe_dir.join("recipe.yaml");

        let opts = BuildOpts {
            recipe_dir: Some(recipe_dir.into()),
            // // Good if you want to try out recipe for different platform, since we are not building them anyway.
            // build_platform: Some(rattler_conda_types::Platform::Win64),
            // target_platform: Some(rattler_conda_types::Platform::Win64),
            // host_platform: Some(rattler_conda_types::Platform::Win64),
            no_build_id: true,
            no_test: true,
            common: CommonOpts {
                use_zstd: true,
                use_bz2: true,
                output_dir: Some(artifacts_dir_path.to_path_buf()),
                ..Default::default()
            },
            sandbox_arguments: SandboxArguments {
                sandbox: true,
                allow_network: true,
                ..Default::default()
            },
            continue_on_failure: true,
            ..Default::default()
        };

        let build_data: BuildData = BuildData::from_opts_and_config(opts, None);
        let tool_config: Configuration = get_tool_config(&build_data, &None).unwrap();

        let outputs = get_build_output(&build_data, &recipe_path, &tool_config).await?;

        let mut patchable_outputs = vec![];
        for output in outputs {
            let mut pkg_sources = vec![];
            let sources = output.recipe.sources();
            for source in sources {
                if !source.patches().is_empty() {
                    pkg_sources.push(source.clone())
                }
            }

            if !pkg_sources.is_empty() {
                patchable_outputs.push((
                    build_data.clone(),
                    tool_config.clone(),
                    output,
                    pkg_sources,
                ))
            }
        }

        ensure!(
            !patchable_outputs.is_empty(),
            "no patchable outputs found in package"
        );

        Ok((artifacts_dir, patchable_outputs))
    }

    fn show_dir_difference(git_dir: &Path, custom_dir: &Path) -> miette::Result<String> {
        let mut cmd = Command::new("diff");

        // FIXME: Replace with something else for windows.
        let dir_difference = String::from_utf8(
            cmd.args([
                OsStr::new("-rN"),
                OsStr::new("--color=always"),
                git_dir.as_os_str(),
                custom_dir.as_os_str(),
            ])
            .output()
            .into_diagnostic()?
            .stdout,
        )
        .into_diagnostic()?;

        Ok(dir_difference)
    }

    /// Compare custom patch application with reference git patch application.
    ///
    /// Takes a long time to execute, on my machine it takes around 7
    /// minutes. Require up to several gigabytes of memory available in
    /// temporary files directory.
    ///
    /// Algorithm:
    ///
    /// 1. Create temporary directory which will contain a copy of a work dir.
    /// 2. Copy work dir to the temporary directory.
    /// 3. Patch original work dir using `git apply`.
    /// 4. Patch temporary work dir using custom patch application.
    /// 5. Compare directories.
    #[rstest]
    #[tokio::test]
    async fn test_package_from_conda_forge(
        #[base_dir = "test-data/conda_forge/recipes"]
        #[files("*")]
        // Slow tests
        #[exclude("(root)|(tiledbsoma)|(libmodplug)")]
        // Insane patch format, needs further investigation on why it
        // even works.
        #[exclude("mumps")]
        // Failed to download source
        #[exclude("petsc")]
        recipe_dir: PathBuf,
    ) -> miette::Result<()> {
        let prep = match prepare_package(&recipe_dir).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{}", e);
                return Ok(());
            }
        };
        let (_tmpdir, patchable_outputs) = prep;
        for (_build_data, tool_configuration, output, sources) in patchable_outputs {
            let directories = output.build_configuration.directories;

            let system_tools = SystemTools::new();

            // Just fetch sources without applying patch.
            let _ = fetch_sources(
                &sources,
                &directories,
                &system_tools,
                &tool_configuration,
                |_, _| Ok(()),
            )
            .await
            .into_diagnostic()?;

            // This directory will contain newly fetched sources to which we want to apply patches.
            let original_sources_dir_path = directories.work_dir;
            // Create copy of that directory.
            let copy_sources_dir = tempfile::tempdir().into_diagnostic()?;
            let copy_sources_dir_path = copy_sources_dir.path().to_path_buf();
            CopyDir::new(&original_sources_dir_path, &copy_sources_dir_path)
                .run()
                .into_diagnostic()?;

            // Apply patches to both directories.
            let patches = sources
                .iter()
                .flat_map(|s| s.patches().iter().cloned())
                .collect::<Vec<PathBuf>>();

            let git_res = apply_patches(
                patches.as_slice(),
                &original_sources_dir_path,
                &recipe_dir,
                |wd, p| apply_patch_git(&system_tools, wd, p),
            );

            let custom_res = apply_patches(
                patches.as_slice(),
                &copy_sources_dir_path,
                &recipe_dir,
                apply_patch_custom,
            );

            if let Ok(difference) =
                show_dir_difference(&original_sources_dir_path, &copy_sources_dir_path)
            {
                if !difference.trim().is_empty() {
                    // If we panic on just nonempty difference then
                    // there are 4 more tests failing, because git
                    // does not apply patches. Specifically
                    // `hf_transfer`, `lua`, `nordugrid_arc`,
                    // `openjph`.
                    eprintln!("Directories are different:\n{}", difference);
                }
            }

            assert!(custom_res.is_ok(), "Results:\n{:#?}", [git_res, custom_res]);
        }

        Ok(())
    }
}
