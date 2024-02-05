use fs_err as fs;
use fs_err::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use itertools::Itertools;

use rattler_conda_types::package::PathsJson;
use rattler_conda_types::package::{ArchiveType, PackageFile};
use rattler_conda_types::Platform;
use rattler_package_streaming::write::{
    write_conda_package, write_tar_bz2_package, CompressionLevel,
};

mod file_finder;
mod file_mapper;
mod metadata;
pub use file_finder::Files;
pub use metadata::{create_prefix_placeholder, to_forward_slash_lossy};

use crate::linux;
use crate::macos;
use crate::metadata::Output;
use crate::package_test::write_test_files;
use crate::post_process;

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

    #[error("Failed to relink ELF file: {0}")]
    LinuxRelinkError(#[from] linux::link::RelinkError),

    #[error("Failed to relink MachO file: {0}")]
    MacOSRelinkError(#[from] macos::link::RelinkError),

    #[error("Relink error: {0}")]
    RelinkError(#[from] crate::post_process::relink::RelinkError),

    #[error(transparent)]
    SourceError(#[from] crate::source::SourceError),

    #[error("could not create python entry point: {0}")]
    CannotCreateEntryPoint(String),

    #[error("Linking check error: {0}")]
    LinkingCheckError(#[from] crate::post_process::LinkingCheckError),

    #[error("Failed to compile Python bytecode: {0}")]
    PythonCompileError(String),
}

/// This function copies the license files to the info/licenses folder.
fn copy_license_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Option<Vec<PathBuf>>, PackagingError> {
    if output.recipe.about().license_file.is_empty() {
        Ok(None)
    } else {
        let license_globs = output.recipe.about().license_file.clone();

        let licenses_folder = tmp_dir_path.join("info/licenses/");
        fs::create_dir_all(&licenses_folder)?;

        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.recipe_dir,
            &licenses_folder,
        )
        .with_parse_globs(license_globs.iter().map(AsRef::as_ref))
        .use_gitignore(false)
        .run()?;

        let copied_files_recipe_dir = copy_dir.copied_paths();
        let any_include_matched_recipe_dir = copy_dir.any_include_glob_matched();

        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.work_dir,
            &licenses_folder,
        )
        .with_parse_globs(license_globs.iter().map(AsRef::as_ref))
        .use_gitignore(false)
        .run()?;

        let copied_files_work_dir = copy_dir.copied_paths();
        let any_include_matched_work_dir = copy_dir.any_include_glob_matched();

        let copied_files = copied_files_recipe_dir
            .iter()
            .chain(copied_files_work_dir)
            .map(PathBuf::from)
            .collect::<Vec<PathBuf>>();

        if !any_include_matched_work_dir && !any_include_matched_recipe_dir {
            tracing::warn!("No include glob matched for copying license files");
        }

        if copied_files.is_empty() {
            tracing::warn!("No license files were copied");
        }

        Ok(Some(copied_files))
    }
}

fn write_recipe_folder(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let recipe_folder = tmp_dir_path.join("info/recipe/");
    let recipe_dir = &output.build_configuration.directories.recipe_dir;

    let copy_result = crate::source::copy_dir::CopyDir::new(recipe_dir, &recipe_folder).run()?;

    let mut files = Vec::from(copy_result.copied_paths());
    // write the variant config to the appropriate file
    let variant_config_file = recipe_folder.join("variant_config.yaml");
    let mut variant_config = File::create(&variant_config_file)?;
    variant_config
        .write_all(serde_yaml::to_string(&output.build_configuration.variant)?.as_bytes())?;
    files.push(variant_config_file);

    // TODO(recipe): define how we want to render it exactly!
    let rendered_recipe_file = recipe_folder.join("rendered_recipe.yaml");
    let mut rendered_recipe = File::create(&rendered_recipe_file)?;
    rendered_recipe.write_all(serde_yaml::to_string(&output)?.as_bytes())?;
    files.push(rendered_recipe_file);

    Ok(files)
}

/// Given an output and a set of new files, create a conda package.
/// This function will copy all the files to a temporary directory and then
/// create a conda package from that. Note that the output needs to have its
/// dependencies finalized before calling this function.
///
/// The `local_channel_dir` is the path to the local channel / output directory.
pub fn package_conda(
    output: &Output,
    files: &Files,
) -> Result<(PathBuf, PathsJson), PackagingError> {
    let local_channel_dir = &output.build_configuration.directories.output_dir;
    let packaging_settings = &output.build_configuration.packaging_settings;
    let prefix = &output.build_configuration.directories.host_prefix;

    if output.finalized_dependencies.is_none() {
        return Err(PackagingError::DependenciesNotFinalized);
    }

    let mut tmp = files.to_temp_folder(output)?;

    tracing::info!("Copying done!");

    let dynamic_linking = output.recipe.build().dynamic_linking();
    let relocation_config = dynamic_linking.binary_relocation().unwrap_or_default();

    if output.build_configuration.target_platform != Platform::NoArch
        && !relocation_config.no_relocation()
    {
        let rpath_allowlist = dynamic_linking.rpath_allowlist();
        let mut binaries = tmp.files.clone();
        if let Some(globs) = relocation_config.relocate_paths() {
            binaries.retain(|v| globs.is_match(v));
        }

        post_process::relink::relink(
            &binaries,
            tmp.temp_dir.path(),
            prefix,
            &output.build_configuration.target_platform,
            &dynamic_linking.rpaths(),
            rpath_allowlist,
        )?;

        post_process::linking_checks(output, &binaries)?;
    }

    tmp.add_files(post_process::python::python(
        output,
        &tmp.files,
        tmp.temp_dir.path(),
    )?);

    tracing::info!("Relink done!");

    let info_folder = tmp.temp_dir.path().join("info");

    tmp.add_files(output.write_metadata(tmp.temp_dir.path(), &tmp.files)?);

    // TODO move things below also to metadata.rs
    if let Some(license_files) = copy_license_files(output, tmp.temp_dir.path())? {
        tmp.add_files(license_files);
    }

    if output.build_configuration.store_recipe {
        let recipe_files = write_recipe_folder(output, tmp.temp_dir.path())?;
        tmp.add_files(recipe_files);
    }

    let test_files = write_test_files(output, tmp.temp_dir.path())?;
    tmp.add_files(test_files);

    // create any entry points or link.json for noarch packages
    if output.recipe.build().noarch().is_python() {
        let link_json = File::create(info_folder.join("link.json"))?;
        serde_json::to_writer_pretty(link_json, &output.link_json()?)?;
        tmp.add_files(vec![info_folder.join("link.json")]);
    } else {
        let entry_points = post_process::python::create_entry_points(output, tmp.temp_dir.path())?;
        tmp.add_files(entry_points);
    }

    // print sorted files
    tracing::info!("\nFiles in package:\n");
    tmp.files
        .iter()
        .map(|x| x.strip_prefix(tmp.temp_dir.path()))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .sorted()
        .for_each(|f| tracing::info!("  - {}", f.to_string_lossy()));

    let output_folder =
        local_channel_dir.join(output.build_configuration.target_platform.to_string());
    tracing::info!("Creating target folder {:?}", output_folder);

    fs::create_dir_all(&output_folder)?;

    let identifier = output
        .identifier()
        .ok_or(PackagingError::BuildStringNotSet)?;
    let out_path = output_folder.join(format!(
        "{}{}",
        identifier,
        packaging_settings.archive_type.extension()
    ));
    let file = File::create(&out_path)?;

    match packaging_settings.archive_type {
        ArchiveType::TarBz2 => {
            write_tar_bz2_package(
                file,
                tmp.temp_dir.path(),
                &tmp.files.iter().cloned().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                Some(&output.build_configuration.timestamp),
            )?;
        }
        ArchiveType::Conda => {
            // This is safe because we're just putting it together before
            write_conda_package(
                file,
                tmp.temp_dir.path(),
                &tmp.files.iter().cloned().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                packaging_settings.compression_threads,
                &identifier,
                Some(&output.build_configuration.timestamp),
            )?;
        }
    }

    let paths_json = PathsJson::from_path(info_folder.join("paths.json"))?;
    Ok((out_path, paths_json))
}

#[cfg(test)]
mod test {
    use super::metadata::create_prefix_placeholder;

    #[test]
    fn detect_prefix() {
        let test_data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/binary_files/binary_file_fallback");
        let prefix = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        create_prefix_placeholder(&test_data, prefix).unwrap();
    }
}
