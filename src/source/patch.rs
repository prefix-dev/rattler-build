//! Functions for applying patches to a work directory.
use crate::system_tools::{SystemTools, Tool};

use super::SourceError;

use std::io::Write;
use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use diffy::Patch;
use fs_err::File;
use itertools::Itertools;

fn parse_patches(patches: &Vec<Patch<[u8]>>) -> HashSet<PathBuf> {
    let mut affected_files = HashSet::new();

    for patch in patches {
        if let Some(p) = patch
            .original()
            .and_then(|p| std::str::from_utf8(p).ok())
            .filter(|p| p.trim() != "/dev/null")
            .map(PathBuf::from)
        {
            affected_files.insert(p);
        }
        if let Some(p) = patch
            .modified()
            .and_then(|p| std::str::from_utf8(p).ok())
            .filter(|p| p.trim() != "/dev/null")
            .map(PathBuf::from)
        {
            affected_files.insert(p);
        }
    }

    affected_files
}

fn patches_from_bytes(input: &[u8]) -> Result<Vec<Patch<'_, [u8]>>, diffy::ParsePatchError> {
    diffy::patches_from_bytes_with_config(
        input,
        diffy::ParserConfig {
            hunk_strategy: diffy::HunkRangeStrategy::Recount,
        },
    )
}

fn apply(base_image: &[u8], patch: &Patch<'_, [u8]>) -> Result<Vec<u8>, diffy::ApplyError> {
    diffy::apply_bytes_with_config(
        base_image,
        patch,
        &diffy::FuzzyConfig {
            max_fuzz: 2,
            ignore_whitespace: true,
            ignore_case: false,
        },
    )
}

// Returns number by which all patch paths must be stripped to be
// successfully applied, or returns and error if no such number could
// be determined.
fn guess_strip_level(patch: &Vec<Patch<[u8]>>, work_dir: &Path) -> Result<usize, SourceError> {
    // There is no /dev/null in here by construction from `parse_patches`.
    let patched_files = parse_patches(patch);

    let max_components = patched_files
        .iter()
        .map(|p| p.components().count())
        .max()
        .unwrap_or(0);

    for strip_level in 0..max_components {
        let all_paths_exist = patched_files.iter().all(|p| {
            let path: PathBuf = p.components().skip(strip_level).collect();
            work_dir.join(path).exists()
        });
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
}

fn custom_patch_stripped_paths(
    patch: &Patch<'_, [u8]>,
    strip_level: usize,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let original = (patch.original(), patch.modified());
    let stripped = (
        // XXX: Probably absolute paths should be checked as well. But
        // it is highly unlikely to meet them in patches, so we ignore
        // that for now.
        original
            .0
            .and_then(|p| std::str::from_utf8(p).ok())
            .and_then(|p| {
                (p.trim() != "/dev/null").then(|| {
                    PathBuf::from(p)
                        .components()
                        .skip(strip_level)
                        .collect::<PathBuf>()
                })
            }),
        original
            .1
            .and_then(|p| std::str::from_utf8(p).ok())
            .and_then(|p| {
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

fn write_patch_content(content: &[u8], path: &Path) -> Result<(), SourceError> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent).map_err(SourceError::Io)?;
    }

    // We want to be able to write to file.
    if path.exists() {
        let mut perms = fs_err::metadata(path)
            .map_err(SourceError::Io)?
            .permissions();
        if perms.readonly() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let user_write = 0o200;
                perms.set_mode(perms.mode() | user_write);
            }
            #[cfg(not(unix))]
            {
                // Assume this means windows
                perms.set_readonly(false);
            }
            fs_err::set_permissions(path, perms).map_err(SourceError::Io)?;
        }
    }

    let mut new_file = File::create(path).map_err(SourceError::Io)?;
    new_file.write_all(content).map_err(SourceError::Io)?;

    Ok(())
}

#[cfg(windows)]
fn temp_copy<P: AsRef<Path>>(src_path: P) -> std::io::Result<tempfile::NamedTempFile> {
    let mut src = File::open(src_path.as_ref())?;
    let mut tmp = tempfile::NamedTempFile::new()?;
    std::io::copy(&mut src, &mut tmp)?;
    Ok(tmp)
}

#[allow(dead_code)]
pub(crate) fn apply_patch_gnu(
    system_tools: &SystemTools,
    work_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), SourceError> {
    let patch_file_content = fs_err::read(patch_file_path).map_err(SourceError::Io)?;

    let patches = patches_from_bytes(&patch_file_content)
        .map_err(|_| SourceError::PatchParseFailed(patch_file_path.to_path_buf()))?;
    let strip_level = guess_strip_level(&patches, work_dir)?;

    tracing::debug!("Patch {} will be applied", patch_file_path.display());

    #[cfg(windows)]
    let patch_tmp = temp_copy(patch_file_path)?;
    #[cfg(windows)]
    let patch_file_path = patch_tmp.path();

    let mut tool = system_tools
        .call(Tool::Patch)
        .map_err(|_| SourceError::PatchExeNotFound)?;
    let cmd_builder = tool
        .arg(format!("-p{}", strip_level))
        .arg("--no-backup-if-mismatch")
        .arg("-i")
        .arg(String::from(patch_file_path.to_string_lossy()))
        .arg("-d")
        .arg(String::from(work_dir.to_string_lossy()));
    let output = cmd_builder.output()?;

    if !output.status.success() {
        return Err(SourceError::PatchFailed(format!(
            "{}\n`patch` failed with a combination of flags.\n\n{}",
            patch_file_path.display(),
            {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                format!(
                    "With the the command:\n\n\t{} {}\n\nThe stdout was:\n\n\t{}\n\nThe stderr was:\n\n\t{}\n\n",
                    cmd_builder.get_program().to_string_lossy(),
                    cmd_builder
                        .get_args()
                        .map(OsStr::to_string_lossy)
                        .format(" "),
                    stdout.lines().format("\n\t"),
                    stderr.lines().format("\n\t")
                )
            }
        )));
    }

    Ok(())
}

pub(crate) fn apply_patch_custom(
    work_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), SourceError> {
    let patch_file_content = fs_err::read(patch_file_path).map_err(SourceError::Io)?;

    let patches = patches_from_bytes(&patch_file_content)
        .map_err(|_| SourceError::PatchParseFailed(patch_file_path.to_path_buf()))?;
    let strip_level = guess_strip_level(&patches, work_dir)?;

    for patch in patches {
        let file_paths = custom_patch_stripped_paths(&patch, strip_level);
        let absolute_file_paths = (
            file_paths.0.map(|o| work_dir.join(&o)),
            file_paths.1.map(|m| work_dir.join(&m)),
        );

        tracing::debug!(
            "Patch will be applied:\n\tFrom: {:#?}\n\tTo:{:#?}",
            absolute_file_paths.0,
            absolute_file_paths.1
        );

        match absolute_file_paths {
            (None, None) => continue,
            (None, Some(m)) => {
                let new_file_content = apply(&[], &patch).map_err(SourceError::PatchApplyError)?;
                write_patch_content(&new_file_content, &m)?;
            }
            (Some(o), None) => {
                fs_err::remove_file(work_dir.join(o)).map_err(SourceError::Io)?;
            }
            (Some(o), Some(m)) => {
                let old_file_content = fs_err::read(&o).map_err(SourceError::Io)?;

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

#[cfg(test)]
mod tests {
    use crate::{
        SystemTools, get_build_output, get_tool_config,
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
    use miette::IntoDiagnostic;
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
                fs_err::read(&patch_path).expect("Could not read file contents");
            let _ = patches_from_bytes(&patch_file_content).expect("Failed to parse patch file");

            println!("Parsing patch: {}", patch_path.display());
        }
    }

    #[test]
    fn get_affected_files() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patch_application/patches");

        let patch_file_content =
            fs_err::read(patches_dir.join("test.patch")).expect("Could not read file contents");
        let patches = patches_from_bytes(&patch_file_content).expect("Failed to parse patch file");

        let patched_paths = parse_patches(&patches);
        assert_eq!(patched_paths.len(), 2);
        assert!(patched_paths.contains(&PathBuf::from("a/text.md")));
        assert!(patched_paths.contains(&PathBuf::from("b/text.md")));

        let patch_file_content =
            fs_err::read(patches_dir.join("0001-increase-minimum-cmake-version.patch"))
                .expect("Could not read file contents");
        let patches = patches_from_bytes(&patch_file_content).expect("Failed to parse patch file");
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
            apply_patch_custom,
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
            apply_patch_custom,
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
            apply_patch_custom,
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

        // Apply the patches in the working directory
        apply_patches(
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
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

        Ok((artifacts_dir, patchable_outputs))
    }

    fn show_dir_difference(git_dir: &Path, custom_dir: &Path) -> miette::Result<String> {
        let mut cmd = Command::new("diff");

        let dir_difference = String::from_utf8(
            cmd.args([
                OsStr::new("-rNu"),
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
    #[ignore]
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
        // GNU patch fails and diffy succeeds, seemingly correctly from the diff output.
        #[exclude("(fastjet-cxx)|(fenics-)|(love2d)|(flask-security-too)")]
        // Parse fails, since createrepo-c/438.patch contains two mail
        // messages in one file. Fix postponed until parser
        // reimplemented.
        #[exclude("createrepo_c")]
        recipe_dir: PathBuf,
    ) -> miette::Result<()> {
        let prep = prepare_package(&recipe_dir).await?;
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
            for source in sources {
                let patches = source.patches().to_vec();
                let target_directory = source.target_directory();

                let (original_source_dir_path, patched_source_dir_path) = match target_directory {
                    Some(td) => (
                        &original_sources_dir_path.join(td),
                        &copy_sources_dir_path.join(td),
                    ),
                    None => (&original_sources_dir_path, &copy_sources_dir_path),
                };

                let gnu_patch_res = apply_patches(
                    patches.as_slice(),
                    original_source_dir_path,
                    &recipe_dir,
                    |wd, p| apply_patch_gnu(&SystemTools::new(), wd, p),
                );

                let custom_res = apply_patches(
                    patches.as_slice(),
                    patched_source_dir_path,
                    &recipe_dir,
                    apply_patch_custom,
                );

                let difference =
                    show_dir_difference(original_source_dir_path, patched_source_dir_path).expect(
                        "Can't show dir difference. Most probably you're missing GNU diff binary.",
                    );

                match (custom_res, gnu_patch_res) {
                    (Ok(_), Ok(_)) => (),
                    (Ok(_), Err(err)) => panic!("Gnu patch failed:\n{}", err),
                    (Err(err), Ok(_)) => panic!("Diffy patch failed:\n{}", err),
                    (Err(cerr), Err(gerr)) => panic!("Both failed:\n{}\n{}", cerr, gerr),
                }

                if !difference.trim().is_empty() {
                    // If we panic on just nonempty difference then
                    // there are 4 more tests failing, because git
                    // does not apply patches. Specifically
                    // `hf_transfer`, `lua`, `nordugrid_arc`,
                    // `openjph`.
                    panic!("Directories are different:\n{}", difference);
                }
            }
        }

        Ok(())
    }
}
