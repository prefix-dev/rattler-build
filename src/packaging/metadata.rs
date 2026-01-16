//! Functions to write and create metadata from a given output

#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStrExt;
use std::{
    borrow::Cow,
    collections::HashSet,
    ops::Deref,
    path::{Path, PathBuf},
};

use content_inspector::ContentType;
use fs_err as fs;
use fs_err::File;
use itertools::Itertools;
use rattler_conda_types::{
    ChannelUrl, NoArchType, Platform,
    package::{
        AboutJson, FileMode, IndexJson, LinkJson, NoArchLinks, PackageFile, PathType, PathsEntry,
        PathsJson, PrefixPlaceholder, PythonEntryPoints, RunExportsJson,
    },
};
use rattler_digest::{compute_bytes_digest, compute_file_digest};
use rayon::prelude::*;
use url::Url;

use super::{PackagingError, TempFiles};
use crate::{hash::HashInput, metadata::Output, recipe::parser::PrefixDetection};

/// Safely check if a symlink resolves to a regular file, with basic loop protection
fn is_symlink_to_file(path: &Path) -> bool {
    // Simple approach: try to canonicalize the path, which handles cycles gracefully
    // If canonicalization fails (due to cycles or missing targets), assume it's not a file
    match path.canonicalize() {
        Ok(canonical_path) => canonical_path.is_file(),
        Err(_) => false, // Could be cycle, missing target, or permission issue
    }
}

/// Detect if the file contains the prefix in binary mode.
#[allow(unused_variables)]
pub fn contains_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
    // Convert the prefix to a Vec<u8> for binary comparison
    // TODO on Windows check both ascii and utf-8 / 16?
    #[cfg(target_family = "windows")]
    {
        tracing::debug!("Windows is not supported yet for binary prefix checking.");
        Ok(false)
    }

    #[cfg(target_family = "unix")]
    {
        let prefix_bytes = prefix.as_os_str().as_bytes().to_vec();

        // Open the file
        let file = File::open(file_path)?;

        // Read the file's content
        let data = unsafe { memmap2::Mmap::map(&file) }?;

        // Check if the content contains the prefix bytes with memchr
        let contains_prefix = memchr::memmem::find_iter(data.as_ref(), &prefix_bytes)
            .next()
            .is_some();

        Ok(contains_prefix)
    }
}

/// This function requires we know the file content we are matching against is
/// UTF-8 In case the source is non utf-8 it will fail with a read error
pub fn contains_prefix_text(
    file_path: &Path,
    prefix: &Path,
) -> Result<Option<String>, PackagingError> {
    // Open the file
    let file = File::open(file_path)?;

    // mmap the file
    let mmap = unsafe { memmap2::Mmap::map(&file)? };

    // Check if the content contains the prefix with memchr
    let prefix_string = prefix.to_string_lossy().to_string();
    let mut detected_prefix = None;
    if memchr::memmem::find_iter(mmap.as_ref(), &prefix_string)
        .next()
        .is_some()
    {
        detected_prefix = Some(prefix_string);
    }

    // On Windows we always also need to check for forward slashes.
    // This also includes `noarch` packages that are built on Windows.
    if cfg!(windows) {
        use crate::utils::to_forward_slash_lossy;
        // absolute and unc paths will break but it,
        // will break either way as C:/ can't be converted
        // to something meaningful in unix either way
        let forward_slash: Cow<'_, str> = to_forward_slash_lossy(prefix);

        if memchr::memmem::find_iter(mmap.as_ref(), forward_slash.deref())
            .next()
            .is_some()
        {
            if detected_prefix.is_some() {
                tracing::error!(
                    "File {file_path:?} contains the prefix with both forward- and backslashes. This is not supported and can lead to issues.\n  Prefix: {prefix:?}",
                );
                return Err(PackagingError::MixedPrefixPlaceholders(
                    file_path.to_path_buf(),
                ));
            }

            return Ok(Some(forward_slash.to_string()));
        }
    }

    Ok(detected_prefix)
}

/// Create a prefix placeholder object for the given file and prefix.
/// This function will also search in the file for the prefix and determine if
/// the file is binary or text.
pub fn create_prefix_placeholder(
    target_platform: &Platform,
    file_path: &Path,
    prefix: &Path,
    encoded_prefix: &Path,
    content_type: &ContentType,
    prefix_detection: &PrefixDetection,
) -> Result<Option<PrefixPlaceholder>, PackagingError> {
    // exclude pyc and pyo files from prefix replacement
    if let Some(ext) = file_path.extension()
        && (ext == "pyc" || ext == "pyo")
    {
        return Ok(None);
    }

    let relative_path = file_path.strip_prefix(prefix)?;
    if prefix_detection.ignore.is_match(relative_path) {
        tracing::info!("Ignoring prefix-detection for file: {:?}", relative_path);
        return Ok(None);
    }

    let force_binary = &prefix_detection.force_file_type.binary;
    let force_text = &prefix_detection.force_file_type.text;

    let forced_file_type = if force_binary.is_match(relative_path) {
        tracing::info!(
            "Forcing binary prefix replacement mode for file: {:?}",
            relative_path
        );
        Some(FileMode::Binary)
    } else if force_text.is_match(relative_path) {
        tracing::info!(
            "Forcing text prefix replacement mode for file: {:?}",
            relative_path
        );
        Some(FileMode::Text)
    } else {
        None
    };

    let mut has_prefix = None;
    // treat everything except for utf8 / utf8-bom as binary for now!
    let detected_is_text = content_type.is_text()
        && matches!(content_type, ContentType::UTF_8 | ContentType::UTF_8_BOM);

    // Even if we force the replacement mode to be text we still cannot handle it
    // like a text file since it likely contains NULL bytes etc.
    let file_mode = if detected_is_text && forced_file_type != Some(FileMode::Binary) {
        match contains_prefix_text(file_path, encoded_prefix) {
            Ok(Some(prefix)) => {
                has_prefix = Some(prefix);
                FileMode::Text
            }
            Ok(None) => FileMode::Text,
            Err(PackagingError::IoError(ioe)) if ioe.kind() == std::io::ErrorKind::InvalidData => {
                FileMode::Binary
            }
            Err(e) => return Err(e),
        }
    } else {
        FileMode::Binary
    };

    if file_mode == FileMode::Binary {
        if prefix_detection.ignore_binary_files {
            tracing::info!(
                "Ignoring binary file for prefix-replacement: {:?}",
                relative_path
            );
            return Ok(None);
        }

        if target_platform.is_windows() {
            tracing::debug!(
                "Binary prefix replacement is not performed fors Windows: {:?}",
                relative_path
            );
            return Ok(None);
        }

        if contains_prefix_binary(file_path, encoded_prefix)? {
            has_prefix = Some(encoded_prefix.to_string_lossy().to_string());
        }
    }

    let file_mode = forced_file_type.unwrap_or(file_mode);
    Ok(has_prefix.map(|placeholder| PrefixPlaceholder {
        file_mode,
        placeholder,
    }))
}

/// Clean credentials out of a channel url and return the string representation
pub fn clean_url(url: &ChannelUrl) -> String {
    let mut url: Url = url.url().clone().into();
    // remove credentials from the url
    url.set_username("").ok();
    url.set_password(None).ok();

    // remove `/t/<TOKEN>` from the url if present
    let segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.collect())
        .unwrap_or_default();

    if segments.len() > 2 && segments[0] == "t" {
        let new_path = segments[2..].join("/");
        url.set_path(&new_path);
    }

    url.to_string()
}

impl Output {
    /// Create the run_exports.json file for the given output.
    pub fn run_exports_json(&self) -> Result<&RunExportsJson, PackagingError> {
        Ok(&self
            .finalized_dependencies
            .as_ref()
            .ok_or(PackagingError::DependenciesNotFinalized)?
            .run
            .run_exports)
    }

    /// Returns the contents of the `hash_input.json` file.
    pub fn hash_input(&self) -> HashInput {
        HashInput::from_variant(&self.build_configuration.variant)
    }

    /// Create the about.json file for the given output.
    pub fn about_json(&self) -> AboutJson {
        let recipe = &self.recipe;

        AboutJson {
            home: recipe
                .about()
                .homepage
                .clone()
                .map(|s| vec![s])
                .unwrap_or_default(),
            license: recipe.about().license.as_ref().map(|l| l.to_string()),
            license_family: recipe.about().license_family.clone(),
            summary: recipe.about().summary.clone(),
            description: recipe.about().description.clone(),
            doc_url: recipe
                .about()
                .documentation
                .clone()
                .map(|url| vec![url])
                .unwrap_or_default(),
            dev_url: recipe
                .about()
                .repository
                .as_ref()
                .map(|url| vec![url.clone()])
                .unwrap_or_default(),
            // TODO ?
            source_url: None,
            channels: self
                .build_configuration
                .channels
                .iter()
                .map(clean_url)
                .collect(),
            extra: self.extra_meta.clone().unwrap_or_default(),
        }
    }

    /// Create the contents of the index.json file for the given output.
    pub fn index_json(&self) -> Result<IndexJson, PackagingError> {
        let recipe = &self.recipe;
        let target_platform = self.target_platform();

        let arch = target_platform.arch().map(|a| a.to_string());
        let platform = target_platform.only_platform().map(|p| p.to_string());

        let finalized_dependencies = self
            .finalized_dependencies
            .as_ref()
            .ok_or(PackagingError::DependenciesNotFinalized)?;

        // Track features are exclusively used to down-prioritize packages
        // Each feature contributes "1 point" to the down-prioritization. So we add a
        // feature for each down-prioritization level.
        let track_features = self
            .recipe
            .build()
            .variant()
            .down_prioritize_variant
            .map(|down_prioritize| {
                let mut track_features = Vec::new();
                for i in 0..down_prioritize.abs() {
                    track_features.push(format!("{}-p-{}", self.name().as_normalized(), i));
                }
                track_features
            })
            .unwrap_or_default();

        if recipe.build().python().site_packages_path.is_some() {
            // check that the package name is Python, otherwise fail
            if self.name().as_normalized() != "python" {
                return Err(PackagingError::InvalidMetadata("Cannot set python_site_packages_path for a package that is not called `python`".to_string()));
            }
        }

        // Support CEP-20 / ABI3 packages
        let noarch = if self.recipe.build().is_python_version_independent() {
            NoArchType::python()
        } else {
            *self.recipe.build().noarch()
        };

        if self.name().as_normalized() != self.name().as_source() {
            tracing::warn!(
                "The package name {} is not the same as the source name {}. Normalizing to {}.",
                self.name().as_normalized(),
                self.name().as_source(),
                self.name().as_normalized()
            );
        }

        Ok(IndexJson {
            name: self
                .name()
                .as_normalized()
                .parse()
                .expect("Should always be valid"),
            version: self.version().clone(),
            build: self.build_string().into_owned(),
            build_number: recipe.build().number(),
            arch,
            platform,
            subdir: Some(self.build_configuration.target_platform.to_string()),
            license: recipe.about().license.as_ref().map(|l| l.to_string()),
            license_family: recipe.about().license_family.clone(),
            timestamp: Some(self.build_configuration.timestamp.into()),
            depends: finalized_dependencies
                .run
                .depends
                .iter()
                .map(|dep| dep.spec().to_string())
                .unique()
                .collect(),
            constrains: finalized_dependencies
                .run
                .constraints
                .iter()
                .map(|dep| dep.spec().to_string())
                .unique()
                .collect(),
            noarch,
            track_features,
            features: None,
            python_site_packages_path: recipe.build().python().site_packages_path.clone(),
            purls: None,
            experimental_extra_depends: Default::default(),
        })
    }

    /// This function creates a link.json file for the given output.
    pub fn link_json(&self) -> Result<LinkJson, PackagingError> {
        let entry_points = &self.recipe.build().python().entry_points;
        let noarch_links = PythonEntryPoints {
            entry_points: entry_points.clone(),
        };

        let link_json = LinkJson {
            noarch: NoArchLinks::Python(noarch_links),
            package_metadata_version: 1,
        };

        Ok(link_json)
    }

    /// Create a `paths.json` file structure for the given paths.
    /// Paths should be given as absolute paths under the `path_prefix`
    /// directory. This function will also determine if the file is binary
    /// or text, and if it contains the prefix.
    pub fn paths_json(&self, temp_files: &TempFiles) -> Result<PathsJson, PackagingError> {
        let always_copy_files = self.recipe.build().always_copy_files();

        let mut paths_json = PathsJson {
            paths: Vec::new(),
            paths_version: 1,
        };

        let entries: Vec<Result<Option<PathsEntry>, PackagingError>> = temp_files
            .content_type_map()
            .par_iter()
            .map(|(p, content_type)| {
                let meta = fs::symlink_metadata(p)?;
                let relative_path = p.strip_prefix(temp_files.temp_dir.path())?.to_path_buf();

                if relative_path.starts_with("info") {
                    return Ok(None);
                }

                if !p.exists() {
                    if p.is_symlink() {
                        if let Ok(link_target) = p.read_link() {
                            if link_target.is_relative() {
                                let Some(relative_path_parent) = relative_path.parent() else {
                                    tracing::warn!("could not get parent of symlink {:?}", &p);
                                    return Ok(None);
                                };
                                let resolved_path = temp_files
                                    .encoded_prefix
                                    .join(relative_path_parent)
                                    .join(&link_target);
                                if !resolved_path.exists() {
                                    tracing::warn!(
                                        "symlink target not part of this package: {:?} -> {:?}",
                                        &p,
                                        &link_target
                                    );
                                    // Continue processing the symlink instead of skipping it
                                }
                            } else {
                                tracing::warn!(
                                    "packaging an absolute symlink to outside the prefix {:?} -> {:?}",
                                    &p,
                                    link_target
                                );
                            }
                        } else {
                            tracing::warn!("could not read symlink {:?}", &p);
                        }
                    } else {
                        tracing::warn!("file does not exist: {:?}", &p);
                        return Ok(None);
                    }
                }

                if meta.is_dir() {
                    let mut entries = fs::read_dir(p)?;
                    if entries.next().is_none() {
                        return Ok(Some(PathsEntry {
                            sha256: None,
                            relative_path,
                            path_type: PathType::Directory,
                            prefix_placeholder: None,
                            no_link: false,
                            size_in_bytes: None,
                        }));
                    }
                } else if meta.is_file() {
                    let content_type = content_type.ok_or_else(|| PackagingError::ContentTypeNotFound(p.clone()))?;
                    let prefix_placeholder = create_prefix_placeholder(
                        &self.build_configuration.target_platform,
                        p,
                        temp_files.temp_dir.path(),
                        &temp_files.encoded_prefix,
                        &content_type,
                        self.recipe.build().prefix_detection(),
                    )?;
                    let file_size = meta.len();
                    // Compute SHA256 for files - empty files get empty hash
                    let digest = if file_size > 0 {
                        Some(compute_file_digest::<sha2::Sha256>(p)?)
                    } else {
                        Some(compute_bytes_digest::<sha2::Sha256>(&[]))
                    };
                    let no_link = always_copy_files.is_match(&relative_path);
                    return Ok(Some(PathsEntry {
                        sha256: digest,
                        relative_path,
                        path_type: PathType::HardLink,
                        prefix_placeholder,
                        no_link,
                        size_in_bytes: Some(file_size),
                    }));
                } else if meta.is_symlink() {
                    // For symlinks, compute hash of the target file content if it exists and is within package, otherwise empty digest
                    let digest = if is_symlink_to_file(p) {
                        compute_file_digest::<sha2::Sha256>(p)?
                    } else {
                        compute_bytes_digest::<sha2::Sha256>(&[])
                    };
                    return Ok(Some(PathsEntry {
                        sha256: Some(digest),
                        relative_path,
                        path_type: PathType::SoftLink,
                        prefix_placeholder: None,
                        no_link: false,
                        size_in_bytes: Some(meta.len()),
                    }));
                }
                Ok(None)
            })
            .collect();

        for entry in entries {
            match entry {
                Ok(Some(path_entry)) => paths_json.paths.push(path_entry),
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }

        Ok(paths_json)
    }

    /// Create the metadata for the given output and place it in the temporary
    /// directory
    pub fn write_metadata(
        &self,
        temp_files: &TempFiles,
    ) -> Result<HashSet<PathBuf>, PackagingError> {
        let mut new_files = HashSet::new();
        let root_dir = temp_files.temp_dir.path();
        let info_folder = temp_files.temp_dir.path().join("info");
        fs::create_dir_all(&info_folder)?;

        let paths_json_path = root_dir.join(PathsJson::package_path());
        let paths_json = File::create(&paths_json_path)?;
        serde_json::to_writer_pretty(paths_json, &self.paths_json(temp_files)?)?;
        new_files.insert(paths_json_path);

        let index_json_path = root_dir.join(IndexJson::package_path());
        let index_json = File::create(&index_json_path)?;
        serde_json::to_writer_pretty(index_json, &self.index_json()?)?;
        new_files.insert(index_json_path);

        let hash_input_path = info_folder.join("hash_input.json");
        fs::write(&hash_input_path, self.hash_input().as_bytes())?;
        new_files.insert(hash_input_path);

        let about_json_path = root_dir.join(AboutJson::package_path());
        let about_json = File::create(&about_json_path)?;
        serde_json::to_writer_pretty(about_json, &self.about_json())?;
        new_files.insert(about_json_path);

        let run_exports = self.run_exports_json()?;
        if !run_exports.is_empty() {
            let run_exports_path = root_dir.join(RunExportsJson::package_path());
            let run_exports_json = File::create(&run_exports_path)?;
            serde_json::to_writer_pretty(run_exports_json, &run_exports)?;
            new_files.insert(run_exports_path);
        }

        Ok(new_files)
    }
}

#[cfg(test)]
mod test {
    use content_inspector::ContentType;
    use rattler_conda_types::{ChannelUrl, Platform};
    use url::Url;

    #[cfg(unix)]
    use super::contains_prefix_binary;
    use super::fs;
    use super::{contains_prefix_text, create_prefix_placeholder};
    use crate::{packaging::metadata::clean_url, recipe::parser::PrefixDetection};

    #[test]
    fn detect_prefix() {
        let test_data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/binary_files/binary_file_fallback");
        let prefix = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        create_prefix_placeholder(
            &Platform::Linux64,
            &test_data,
            prefix,
            prefix,
            &ContentType::BINARY,
            &PrefixDetection::default(),
        )
        .unwrap();
    }

    #[test]
    fn test_clean_url() {
        let url = ChannelUrl::from(Url::parse("https://example.com/t/TOKEN/conda-forge").unwrap());
        let cleaned_url = clean_url(&url);
        assert_eq!(cleaned_url, "https://example.com/conda-forge/");

        // user+password@host
        let url =
            ChannelUrl::from(Url::parse("https://user:password@foobar.com/mychannel").unwrap());
        let cleaned_url = clean_url(&url);
        assert_eq!(cleaned_url, "https://foobar.com/mychannel/");
    }

    #[cfg(unix)]
    use std::io::Write;

    #[test]
    fn contains_prefix_text_positive() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix_path = tmp.path().join("my_prefix");
        let file_path = tmp.path().join("example.txt");
        let content = format!("This file lives in {} directory", prefix_path.display());
        fs::write(&file_path, content).unwrap();

        let found = contains_prefix_text(&file_path, &prefix_path).unwrap();
        assert_eq!(found, Some(prefix_path.to_string_lossy().to_string()));
    }

    #[test]
    fn contains_prefix_text_negative() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix_path = tmp.path().join("absent_prefix");
        let file_path = tmp.path().join("note.txt");
        fs::write(&file_path, "nothing to see here").unwrap();
        let found = contains_prefix_text(&file_path, &prefix_path).unwrap();
        assert!(found.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn contains_prefix_binary_unix() {
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix_binary");
        let file_path = tmp.path().join("binfile.bin");

        // write arbitrary binary data including the prefix bytes
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(b"random bytes ").unwrap();
        f.write_all(prefix.as_os_str().as_bytes()).unwrap();
        f.write_all(b" tail").unwrap();
        drop(f);

        assert!(contains_prefix_binary(&file_path, &prefix).unwrap());

        // File without the prefix should return false
        let other = tmp.path().join("noprefix.bin");
        fs::write(&other, b"just binary").unwrap();
        assert!(!contains_prefix_binary(&other, &prefix).unwrap());
    }
}
