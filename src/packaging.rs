//! This module contains the functions to package a conda package from a given
//! output.
use std::{
    collections::HashSet,
    io::Write,
    path::{Component, Path, PathBuf},
};

use fs_err as fs;
use fs_err::File;
use rattler_conda_types::{
    package::{ArchiveType, PackageFile, PathsJson},
    Platform,
};
use rattler_package_streaming::write::{
    write_conda_package, write_tar_bz2_package, CompressionLevel,
};

mod file_finder;
mod file_mapper;
mod metadata;
pub use file_finder::{content_type, Files, TempFiles};
pub use metadata::{contains_prefix_binary, contains_prefix_text, create_prefix_placeholder};
use tempfile::NamedTempFile;

use crate::{
    metadata::Output,
    package_test::write_test_files,
    post_process,
    recipe::parser::GlobVec,
    source::{self, copy_dir},
    tool_configuration,
};

#[allow(missing_docs)]
#[derive(Debug, thiserror::Error)]
pub enum PackagingError {
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_yaml::Error),

    #[error("Failed to build glob from pattern")]
    GlobError(#[from] globset::Error),

    #[error("Build String is not yet set")]
    BuildStringNotSet,

    #[error("Dependencies are not yet finalized / resolved")]
    DependenciesNotFinalized,

    #[error("Could not open or create, or write to file")]
    IoError(#[from] std::io::Error),

    #[error("Could not strip a prefix from a Path")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("Could not serialize JSON: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Could not run walkdir: {0}")]
    WalkDirError(#[from] walkdir::Error),

    #[error("Failed to parse version {0}")]
    VersionParseError(#[from] rattler_conda_types::ParseVersionError),

    #[error(transparent)]
    RelinkError(#[from] crate::post_process::relink::RelinkError),

    #[error(transparent)]
    SourceError(#[from] source::SourceError),

    #[error("could not create python entry point: {0}")]
    CannotCreateEntryPoint(String),

    #[error("linking check error: {0}")]
    LinkingCheckError(#[from] crate::post_process::checks::LinkingCheckError),

    #[error("Failed to compile Python bytecode: {0}")]
    PythonCompileError(String),

    #[error("Failed to find content type for file: {0:?}")]
    ContentTypeNotFound(PathBuf),

    #[error("No license files were copied")]
    LicensesNotFound,

    #[error("Invalid Metadata: {0}")]
    InvalidMetadata(String),

    #[error("Invalid MenuInst schema file: {0} - {1}")]
    InvalidMenuInstSchema(PathBuf, serde_json::Error),
}

/// This function copies the license files to the info/licenses folder.
/// License files are selected from the recipe directory and the source (work) folder.
/// If the same file is found in both locations, the file from the recipe directory is used.
fn copy_license_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Option<HashSet<PathBuf>>, PackagingError> {
    if output.recipe.about().license_file.is_empty() {
        Ok(None)
    } else {
        let licenses_folder = tmp_dir_path.join("info/licenses/");
        fs::create_dir_all(&licenses_folder)?;

        let copy_dir = copy_dir::CopyDir::new(
            &output.build_configuration.directories.work_dir,
            &licenses_folder,
        )
        .with_globvec(&output.recipe.about().license_file)
        .use_gitignore(false)
        .run()?;

        let copied_files_work_dir = copy_dir.copied_paths();
        let any_include_matched_recipe_dir = copy_dir.any_include_glob_matched();

        let copy_dir = copy_dir::CopyDir::new(
            &output.build_configuration.directories.recipe_dir,
            &licenses_folder,
        )
        .with_globvec(&output.recipe.about().license_file)
        .use_gitignore(false)
        .overwrite(true)
        .run()?;

        let copied_files_recipe_dir = copy_dir.copied_paths();
        let any_include_matched_work_dir = copy_dir.any_include_glob_matched();

        // if a file was copied from the recipe dir, and the work dir, we should
        // issue a warning
        for file in copied_files_recipe_dir {
            if copied_files_work_dir.contains(file) {
                let warn_str = format!("License file from source directory was overwritten by license file from recipe folder ({})", file.display());
                tracing::warn!(warn_str);
                output.record_warning(&warn_str);
            }
        }

        let copied_files = copied_files_recipe_dir
            .iter()
            .chain(copied_files_work_dir)
            .map(PathBuf::from)
            .collect::<HashSet<PathBuf>>();

        if !any_include_matched_work_dir && !any_include_matched_recipe_dir {
            let warn_str = "No include glob matched for copying license files";
            tracing::warn!(warn_str);
            output.record_warning(warn_str);
        }

        if copied_files.is_empty() {
            Err(PackagingError::LicensesNotFound)
        } else {
            Ok(Some(copied_files))
        }
    }
}

fn write_recipe_folder(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let recipe_folder = tmp_dir_path.join("info/recipe/");
    let recipe_dir = &output.build_configuration.directories.recipe_dir;
    let recipe_path = &output.build_configuration.directories.recipe_path;
    let output_dir = &output.build_configuration.directories.output_dir;

    let mut copy_builder = copy_dir::CopyDir::new(recipe_dir, &recipe_folder)
        .use_gitignore(true)
        .ignore_hidden_files(true);

    // if the output dir is inside the same directory as the recipe, then we
    // need to ignore the output dir when copying
    if let Ok(ignore_output) = output_dir.strip_prefix(recipe_dir) {
        tracing::info!(
            "Ignoring output dir in recipe folder: {}",
            output_dir.to_string_lossy()
        );
        let output_dir_glob = format!("{}/**", ignore_output.to_string_lossy());
        let glob_vec = GlobVec::from_vec(vec![], Some(vec![&output_dir_glob]));
        copy_builder = copy_builder.with_globvec(&glob_vec);
    }

    let copy_result = copy_builder.run()?;

    let mut files = Vec::from(copy_result.copied_paths());

    // Make sure that the recipe file is "recipe.yaml" in `info/recipe/`
    if recipe_path.file_name() != Some("recipe.yaml".as_ref()) {
        if let Some(name) = recipe_path.file_name() {
            fs::rename(recipe_folder.join(name), recipe_folder.join("recipe.yaml"))?;
            // Update the existing entry with the new recipe file.
            if let Some(pos) = files.iter().position(|x| x == &recipe_folder.join(name)) {
                files[pos] = recipe_folder.join("recipe.yaml");
            }
        }
    }

    // write the variant config to the appropriate file
    let variant_config_file = recipe_folder.join("variant_config.yaml");
    let mut variant_config = File::create(&variant_config_file)?;
    variant_config
        .write_all(serde_yaml::to_string(&output.build_configuration.variant)?.as_bytes())?;
    files.push(variant_config_file);

    // Write out the "rendered" recipe as well (the recipe with all the variables
    // replaced with their values)
    let rendered_recipe_file = recipe_folder.join("rendered_recipe.yaml");
    let mut rendered_recipe = File::create(&rendered_recipe_file)?;
    rendered_recipe.write_all(serde_yaml::to_string(&output)?.as_bytes())?;
    files.push(rendered_recipe_file);

    Ok(files)
}

struct ProgressBar {
    progress_bar: indicatif::ProgressBar,
}

impl rattler_package_streaming::write::ProgressBar for ProgressBar {
    fn set_progress(&mut self, progress: u64, message: &str) {
        self.progress_bar.set_position(progress);
        self.progress_bar.set_message(message.to_string());
    }

    fn set_total(&mut self, total: u64) {
        self.progress_bar.set_length(total);
    }
}

/// Given an output and a set of new files, create a conda package.
/// This function will copy all the files to a temporary directory and then
/// create a conda package from that. Note that the output needs to have its
/// dependencies finalized before calling this function.
///
/// The `local_channel_dir` is the path to the local channel / output directory.
pub fn package_conda(
    output: &Output,
    tool_configuration: &tool_configuration::Configuration,
    files: &Files,
) -> Result<(PathBuf, PathsJson), PackagingError> {
    let local_channel_dir = &output.build_configuration.directories.output_dir;
    let packaging_settings = &output.build_configuration.packaging_settings;

    if output.finalized_dependencies.is_none() {
        return Err(PackagingError::DependenciesNotFinalized);
    }

    let mut tmp = files.to_temp_folder(output)?;

    tracing::info!("Copying done!");

    post_process::relink::relink(&tmp, output)?;

    post_process::menuinst::menuinst(&tmp)?;

    tmp.add_files(post_process::python::python(&tmp, output)?);

    post_process::regex_replacements::regex_post_process(&tmp, output)?;

    tracing::info!("Post-processing done!");

    let info_folder = tmp.temp_dir.path().join("info");

    tracing::info!("Writing test files");
    let test_files = write_test_files(output, tmp.temp_dir.path())?;
    tmp.add_files(test_files);

    tracing::info!("Writing metadata for package");
    tmp.add_files(output.write_metadata(&tmp)?);

    // TODO move things below also to metadata.rs
    tracing::info!("Copying license files");
    if let Some(license_files) = copy_license_files(output, tmp.temp_dir.path())? {
        tmp.add_files(license_files);
    }

    tracing::info!("Copying recipe files");
    if output.build_configuration.store_recipe {
        let recipe_files = write_recipe_folder(output, tmp.temp_dir.path())?;
        tmp.add_files(recipe_files);
    }

    tracing::info!("Creating entry points");
    // create any entry points or link.json for noarch packages
    if output.recipe.build().is_python_version_independent() {
        let link_json = File::create(info_folder.join("link.json"))?;
        serde_json::to_writer_pretty(link_json, &output.link_json()?)?;
        tmp.add_files(vec![info_folder.join("link.json")]);
    }

    // print sorted files
    tracing::info!("\nFiles in package:\n");
    let mut files = tmp
        .files
        .iter()
        .map(|x| x.strip_prefix(tmp.temp_dir.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_by(|a, b| {
        let a_is_info = a.components().next() == Some(Component::Normal("info".as_ref()));
        let b_is_info = b.components().next() == Some(Component::Normal("info".as_ref()));
        match (a_is_info, b_is_info) {
            (true, true) | (false, false) => a.cmp(b),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
        }
    });

    let normalize_path = |p: &Path| -> String { p.display().to_string().replace('\\', "/") };

    files.iter().for_each(|f| {
        if f.components().next() == Some(Component::Normal("info".as_ref())) {
            tracing::info!("  - {}", console::style(normalize_path(f)).dim())
        } else {
            tracing::info!("  - {}", normalize_path(f))
        }
    });

    let output_folder =
        local_channel_dir.join(output.build_configuration.target_platform.to_string());
    tracing::info!("Creating target folder '{}'", output_folder.display());

    fs::create_dir_all(&output_folder)?;

    if let Platform::NoArch = output.build_configuration.target_platform {
        create_empty_build_folder(
            local_channel_dir,
            &output.build_configuration.build_platform.platform,
        )?;
    }

    let identifier = output.identifier();
    let tempfile_in_output = NamedTempFile::new_in(&output_folder)?;

    let final_name = output_folder.join(format!(
        "{}{}",
        identifier,
        packaging_settings.archive_type.extension()
    ));

    tracing::info!("Compressing archive...");

    let progress_bar = tool_configuration.fancy_log_handler.add_progress_bar(
        indicatif::ProgressBar::new(0)
            .with_prefix("Compressing ")
            .with_style(tool_configuration.fancy_log_handler.default_bytes_style()),
    );

    match packaging_settings.archive_type {
        ArchiveType::TarBz2 => {
            write_tar_bz2_package(
                tempfile_in_output.as_file(),
                tmp.temp_dir.path(),
                &tmp.files.iter().cloned().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                Some(&output.build_configuration.timestamp),
                Some(Box::new(ProgressBar { progress_bar })),
            )?;
        }
        ArchiveType::Conda => {
            write_conda_package(
                tempfile_in_output.as_file(),
                tmp.temp_dir.path(),
                &tmp.files.iter().cloned().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                tool_configuration.compression_threads,
                &identifier,
                Some(&output.build_configuration.timestamp),
                Some(Box::new(ProgressBar { progress_bar })),
            )?;
        }
    }

    // Atomically move the file to the final location
    tempfile_in_output
        .persist(&final_name)
        .map_err(|e| e.error)?;
    tracing::info!("Archive written to '{}'", final_name.display());

    let paths_json = PathsJson::from_path(info_folder.join("paths.json"))?;
    Ok((final_name, paths_json))
}

/// When building package for noarch, we don't create another build-platform
/// folder together with noarch but conda-build does
/// because of this we have a failure in conda-smithy CI so we also *mimic* this
/// behaviour until this behaviour is changed
/// https://github.com/conda-forge/conda-forge-ci-setup-feedstock/blob/main/recipe/conda_forge_ci_setup/feedstock_outputs.py#L164
fn create_empty_build_folder(
    local_channel_dir: &Path,
    build_platform: &Platform,
) -> miette::Result<(), PackagingError> {
    let build_output_folder = local_channel_dir.join(build_platform.to_string());

    tracing::info!("Creating empty build folder {:?}", build_output_folder);

    fs::create_dir_all(&build_output_folder)?;

    Ok(())
}

impl Output {
    /// Create a conda package from any new files in the host prefix. Note: the
    /// previous stages should have been completed before calling this
    /// function.
    pub async fn create_package(
        &self,
        tool_configuration: &tool_configuration::Configuration,
    ) -> Result<(PathBuf, PathsJson), PackagingError> {
        let span = tracing::info_span!("Packaging new files");
        let _enter = span.enter();
        let files_after = Files::from_prefix(
            &self.build_configuration.directories.host_prefix,
            self.recipe.build().always_include_files(),
            self.recipe.build().files(),
        )?;

        package_conda(self, tool_configuration, &files_after)
    }
}
