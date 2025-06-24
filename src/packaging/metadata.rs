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
use url::Url;

use super::{PackagingError, TempFiles};
use crate::{hash::HashInput, metadata::Output, recipe::parser::PrefixDetection};

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
    target_platform: &Platform,
) -> Result<Option<String>, PackagingError> {
    // Open the file
    let file = File::open(file_path)?;

    // mmap the file
    let mmap = unsafe { memmap2::Mmap::map(&file)? };

    // Check if the content contains the prefix with memchr
    let prefix_string = prefix.to_string_lossy().to_string();
    if memchr::memmem::find_iter(mmap.as_ref(), &prefix_string)
        .next()
        .is_some()
    {
        return Ok(Some(prefix_string));
    }

    if target_platform.is_windows() {
        use crate::utils::to_forward_slash_lossy;
        // absolute and unc paths will break but it,
        // will break either way as C:/ can't be converted
        // to something meaningful in unix either way
        let forward_slash: Cow<'_, str> = to_forward_slash_lossy(prefix);

        if memchr::memmem::find_iter(mmap.as_ref(), forward_slash.deref())
            .next()
            .is_some()
        {
            return Ok(Some(forward_slash.to_string()));
        }
    }

    Ok(None)
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
    if let Some(ext) = file_path.extension() {
        if ext == "pyc" || ext == "pyo" {
            return Ok(None);
        }
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
        match contains_prefix_text(file_path, encoded_prefix, target_platform) {
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

        let about_json = AboutJson {
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
        };

        about_json
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
        // Each feature contributes "1 point" to the down-priorization. So we add a
        // feature for each down-priorization level.
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
            timestamp: Some(self.build_configuration.timestamp),
            depends: finalized_dependencies
                .run
                .depends
                .iter()
                .map(|dep| dep.spec().to_string())
                .dedup()
                .collect(),
            constrains: finalized_dependencies
                .run
                .constraints
                .iter()
                .map(|dep| dep.spec().to_string())
                .dedup()
                .collect(),
            noarch,
            track_features,
            features: None,
            python_site_packages_path: recipe.build().python().site_packages_path.clone(),
            purls: None,
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

        let sorted = temp_files
            .content_type_map()
            .iter()
            .sorted_by(|(k1, _), (k2, _)| k1.cmp(k2));

        for (p, content_type) in sorted {
            let meta = fs::symlink_metadata(p)?;

            let relative_path = p.strip_prefix(temp_files.temp_dir.path())?.to_path_buf();

            // skip any info files as they are not part of the paths.json
            if relative_path.starts_with("info") {
                continue;
            }

            if !p.exists() {
                if p.is_symlink() {
                    // check if the file is in the prefix
                    if let Ok(link_target) = p.read_link() {
                        if link_target.is_relative() {
                            let Some(relative_path_parent) = relative_path.parent() else {
                                tracing::warn!("could not get parent of symlink {:?}", &p);
                                continue;
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

                                // Think about continuing here or packaging broken symlinks
                                continue;
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
                    continue;
                }
            }

            if meta.is_dir() {
                // check if dir is empty, and only then add it to paths.json
                let mut entries = fs::read_dir(p)?;
                if entries.next().is_none() {
                    let path_entry = PathsEntry {
                        sha256: None,
                        relative_path,
                        path_type: PathType::Directory,
                        prefix_placeholder: None,
                        no_link: false,
                        size_in_bytes: None,
                    };
                    paths_json.paths.push(path_entry);
                }
            } else if meta.is_file() {
                let content_type =
                    content_type.ok_or_else(|| PackagingError::ContentTypeNotFound(p.clone()))?;
                let prefix_placeholder = create_prefix_placeholder(
                    &self.build_configuration.target_platform,
                    p,
                    temp_files.temp_dir.path(),
                    &temp_files.encoded_prefix,
                    &content_type,
                    self.recipe.build().prefix_detection(),
                )?;

                let digest = compute_file_digest::<sha2::Sha256>(p)?;
                let no_link = always_copy_files.is_match(&relative_path);
                paths_json.paths.push(PathsEntry {
                    sha256: Some(digest),
                    relative_path,
                    path_type: PathType::HardLink,
                    prefix_placeholder,
                    no_link,
                    size_in_bytes: Some(meta.len()),
                });
            } else if meta.is_symlink() {
                let digest = if p.is_file() {
                    compute_file_digest::<sha2::Sha256>(p)?
                } else {
                    compute_bytes_digest::<sha2::Sha256>(&[])
                };

                paths_json.paths.push(PathsEntry {
                    sha256: Some(digest),
                    relative_path,
                    path_type: PathType::SoftLink,
                    prefix_placeholder: None,
                    no_link: false,
                    size_in_bytes: Some(meta.len()),
                });
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

    use super::create_prefix_placeholder;
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
}
