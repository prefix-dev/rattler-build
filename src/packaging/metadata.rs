//! Functions to write and create metadata from a given output

use content_inspector::ContentType;
use fs_err as fs;
use fs_err::File;
use itertools::Itertools;
use rattler_conda_types::package::{
    AboutJson, FileMode, IndexJson, LinkJson, NoArchLinks, PathType, PathsEntry, PathsJson,
    PrefixPlaceholder, PythonEntryPoints, RunExportsJson,
};
use rattler_digest::compute_file_digest;
use std::{
    collections::HashSet,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStrExt;

use crate::metadata::Output;

use super::PackagingError;

#[allow(unused_variables)]
fn contains_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
    // Convert the prefix to a Vec<u8> for binary comparison
    // TODO on Windows check both ascii and utf-8 / 16?
    #[cfg(target_family = "windows")]
    {
        tracing::warn!("Windows is not supported yet for binary prefix checking.");
        Ok(false)
    }

    #[cfg(target_family = "unix")]
    {
        let prefix_bytes = prefix.as_os_str().as_bytes().to_vec();

        // Open the file
        let file = File::open(file_path)?;
        let mut buf_reader = BufReader::new(file);

        // Read the file's content
        let mut content = Vec::new();
        buf_reader.read_to_end(&mut content)?;

        // Check if the content contains the prefix bytes
        let contains_prefix = content
            .windows(prefix_bytes.len())
            .any(|window| window == prefix_bytes.as_slice());

        Ok(contains_prefix)
    }
}

/// This function requires we know the file content we are matching against is UTF-8
/// In case the source is non utf-8 it will fail with a read error
fn contains_prefix_text(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
    // Open the file
    let file = File::open(file_path)?;
    let mut buf_reader = BufReader::new(file);

    // Read the file's content
    let mut content = String::new();
    buf_reader.read_to_string(&mut content)?;

    // Check if the content contains the prefix
    let src = prefix.to_string_lossy();
    let contains_prefix = content.contains(&src.to_string());

    #[cfg(target_os = "windows")]
    {
        // absolute and unc paths will break but it,
        // will break either way as C:/ can't be converted
        // to something meaningful in unix either way
        let s = to_forward_slash_lossy(prefix);
        return Ok(contains_prefix || content.contains(s.to_string().as_str()));
    }

    #[cfg(not(target_os = "windows"))]
    Ok(contains_prefix)
}

#[allow(dead_code)]
pub fn to_forward_slash_lossy(path: &Path) -> std::borrow::Cow<'_, str> {
    #[cfg(target_os = "windows")]
    {
        let mut buf = String::new();
        for c in path.components() {
            match c {
                Component::RootDir => { /* root on windows can be skipped */ }
                Component::CurDir => buf.push('.'),
                Component::ParentDir => buf.push_str(".."),
                Component::Prefix(prefix) => {
                    buf.push_str(&prefix.as_os_str().to_string_lossy());
                    continue;
                }
                Component::Normal(s) => buf.push_str(&s.to_string_lossy()),
            }
            // use `/` instead of `\`
            buf.push('/');
        }

        fn ends_with_main_sep(p: &Path) -> bool {
            use std::os::windows::ffi::OsStrExt as _;
            p.as_os_str().encode_wide().last() == Some(std::path::MAIN_SEPARATOR as u16)
        }
        if buf != "/" && !ends_with_main_sep(path) && buf.ends_with('/') {
            buf.pop();
        }

        std::borrow::Cow::Owned(buf)
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string_lossy()
    }
}

/// Create a prefix placeholder object for the given file and prefix.
/// This function will also search in the file for the prefix and determine if the file is binary or text.
pub fn create_prefix_placeholder(
    file_path: &Path,
    prefix: &Path,
) -> Result<Option<PrefixPlaceholder>, PackagingError> {
    // exclude pyc and pyo files from prefix replacement
    if let Some(ext) = file_path.extension() {
        if ext == "pyc" || ext == "pyo" {
            return Ok(None);
        }
    }
    // read first 1024 bytes to determine file type
    let mut file = File::open(file_path)?;
    let mut buffer = [0; 1024];
    let n = file.read(&mut buffer)?;
    let buffer = &buffer[..n];

    let content_type = content_inspector::inspect(buffer);
    let mut has_prefix = None;

    let file_mode = if content_type.is_text()
        // treat everything else as binary for now!
        && matches!(content_type, ContentType::UTF_8 | ContentType::UTF_8_BOM)
    {
        match contains_prefix_text(file_path, prefix) {
            Ok(true) => {
                has_prefix = Some(prefix.to_path_buf());
                FileMode::Text
            }
            Ok(false) => FileMode::Text,
            Err(PackagingError::IoError(ioe)) if ioe.kind() == std::io::ErrorKind::InvalidData => {
                FileMode::Binary
            }
            Err(e) => return Err(e),
        }
    } else {
        FileMode::Binary
    };

    if file_mode == FileMode::Binary && contains_prefix_binary(file_path, prefix)? {
        has_prefix = Some(prefix.to_path_buf());
    }

    Ok(has_prefix.map(|prefix_placeholder| PrefixPlaceholder {
        file_mode,
        placeholder: prefix_placeholder.to_string_lossy().to_string(),
    }))
}

impl Output {
    /// Create the run_exports.json file for the given output.
    pub fn run_exports_json(&self) -> Result<Option<RunExportsJson>, PackagingError> {
        if let Some(run_exports) = &self
            .finalized_dependencies
            .as_ref()
            .ok_or(PackagingError::DependenciesNotFinalized)?
            .run
            .run_exports
        {
            Ok(Some(run_exports.clone()))
        } else {
            Ok(None)
        }
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
            channels: self.build_configuration.channels.clone(),
        };

        about_json
    }

    /// Create the contents of the index.json file for the given output.
    pub fn index_json(&self) -> Result<IndexJson, PackagingError> {
        let recipe = &self.recipe;
        let target_platform = self.build_configuration.target_platform;

        let arch = target_platform.arch().map(|a| a.to_string());
        let platform = target_platform.only_platform().map(|p| p.to_string());

        let finalized_dependencies = self
            .finalized_dependencies
            .as_ref()
            .ok_or(PackagingError::DependenciesNotFinalized)?;

        // Track features are exclusively used to down-prioritize packages
        // Each feature contributes "1 point" to the down-priorization. So we add a feature for each
        // down-priorization level.
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

        Ok(IndexJson {
            name: self.name().clone(),
            version: self.version().parse()?,
            build: self
                .build_string()
                .ok_or(PackagingError::BuildStringNotSet)?
                .to_string(),
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
                .constrains
                .iter()
                .map(|dep| dep.spec().to_string())
                .dedup()
                .collect(),
            noarch: *recipe.build().noarch(),
            track_features,
            features: None,
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
    /// Paths should be given as absolute paths under the `path_prefix` directory.
    /// This function will also determine if the file is binary or text, and if it contains the prefix.
    pub fn paths_json(
        &self,
        paths: &HashSet<PathBuf>,
        path_prefix: &Path,
        encoded_prefix: &Path,
    ) -> Result<PathsJson, PackagingError> {
        let always_copy_files = self.recipe.build().always_copy_files();

        let mut paths_json = PathsJson {
            paths: Vec::new(),
            paths_version: 1,
        };

        for p in itertools::sorted(paths) {
            let meta = fs::symlink_metadata(p)?;

            let relative_path = p.strip_prefix(path_prefix)?.to_path_buf();

            if !p.exists() {
                if p.is_symlink() {
                    tracing::warn!(
                        "Symlink target does not exist: {:?} -> {:?}",
                        &p,
                        fs::read_link(p)?
                    );
                    continue;
                }
                tracing::warn!("File does not exist: {:?} (TODO)", &p);
                continue;
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
                let prefix_placeholder = create_prefix_placeholder(p, encoded_prefix)?;

                let digest = compute_file_digest::<sha2::Sha256>(p)?;
                let no_link = always_copy_files
                    .as_ref()
                    .map(|g| g.is_match(&relative_path))
                    .unwrap_or(false);
                paths_json.paths.push(PathsEntry {
                    sha256: Some(digest),
                    relative_path,
                    path_type: PathType::HardLink,
                    prefix_placeholder,
                    no_link,
                    size_in_bytes: Some(meta.len()),
                });
            } else if meta.file_type().is_symlink() {
                let digest = compute_file_digest::<sha2::Sha256>(p)?;

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

    /// Create the metadata for the given output and place it in the temporary directory
    pub fn write_metadata(
        &self,
        tmp_dir_path: &Path,
        package_files: &HashSet<PathBuf>,
    ) -> Result<HashSet<PathBuf>, PackagingError> {
        let mut new_files = HashSet::new();
        let info_folder = tmp_dir_path.join("info");
        fs::create_dir_all(&info_folder)?;

        let paths_json = File::create(info_folder.join("paths.json"))?;
        let paths_json_struct = self.paths_json(
            &package_files,
            tmp_dir_path,
            &self.build_configuration.directories.host_prefix,
        )?;
        serde_json::to_writer_pretty(paths_json, &paths_json_struct)?;
        new_files.insert(info_folder.join("paths.json"));

        let index_json = File::create(info_folder.join("index.json"))?;
        serde_json::to_writer_pretty(index_json, &self.index_json()?)?;
        new_files.insert(info_folder.join("index.json"));

        let hash_input_json = File::create(info_folder.join("hash_input.json"))?;
        serde_json::to_writer_pretty(hash_input_json, &self.build_configuration.hash.hash_input)?;
        new_files.insert(info_folder.join("hash_input.json"));

        let about_json = File::create(info_folder.join("about.json"))?;
        serde_json::to_writer_pretty(about_json, &self.about_json())?;
        new_files.insert(info_folder.join("about.json"));

        if let Some(run_exports) = self.run_exports_json()? {
            let run_exports_json = File::create(info_folder.join("run_exports.json"))?;
            serde_json::to_writer_pretty(run_exports_json, &run_exports)?;
            new_files.insert(info_folder.join("run_exports.json"));
        }

        let mut variant_config = File::create(info_folder.join("hash_input.json"))?;
        variant_config.write_all(
            serde_json::to_string_pretty(&self.build_configuration.variant)?.as_bytes(),
        )?;

        Ok(new_files)
    }
}
