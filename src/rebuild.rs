//! The rebuild module contains rebuild helper functions.

use fs_err as fs;
use rattler_conda_types::package::ArchiveType;
use std::path::{Path, PathBuf};

/// Extracts a folder from a tar.bz2 archive.
fn folder_from_tar_bz2(
    archive_path: &Path,
    find_path: &Path,
    dest_folder: &Path,
) -> Result<(), std::io::Error> {
    let reader = fs::File::open(archive_path)?;
    let mut archive = rattler_package_streaming::read::stream_tar_bz2(reader);
    archive.set_preserve_permissions(true);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if let Ok(stripped_path) = path.strip_prefix(find_path) {
            let dest_file = dest_folder.join(stripped_path);
            if let Some(parent_folder) = dest_file.parent() {
                if !parent_folder.exists() {
                    fs::create_dir_all(parent_folder)?;
                }
            }
            entry.unpack(dest_file)?;
        }
    }
    Ok(())
}

/// Extracts a folder from a conda archive.
fn folder_from_conda(
    archive_path: &Path,
    find_path: &Path,
    dest_folder: &Path,
) -> Result<(), std::io::Error> {
    let reader = fs::File::open(archive_path)?;

    let mut archive = if find_path.starts_with("info") {
        rattler_package_streaming::seek::stream_conda_info(reader)
            .expect("Could not open conda file")
    } else {
        todo!("Not implemented yet");
    };

    archive.set_preserve_permissions(true);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if let Ok(stripped_path) = path.strip_prefix(find_path) {
            let dest_file = dest_folder.join(stripped_path);
            if let Some(parent_folder) = dest_file.parent() {
                if !parent_folder.exists() {
                    fs::create_dir_all(parent_folder)?;
                }
            }
            entry.unpack(dest_file)?;
        }
    }
    Ok(())
}

/// Extracts a recipe from a package archive to a destination folder.
pub fn extract_recipe(package: &Path, dest_folder: &Path) -> Result<(), std::io::Error> {
    let archive_type = ArchiveType::try_from(package).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "package does not point to valid archive",
        )
    })?;
    let path = PathBuf::from("info/recipe");
    match archive_type {
        ArchiveType::TarBz2 => folder_from_tar_bz2(package, &path, dest_folder)?,
        ArchiveType::Conda => folder_from_conda(package, &path, dest_folder)?,
    };
    Ok(())
}
