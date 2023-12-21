//! Module for fetching sources and applying patches

use std::{
    ffi::OsStr,
    path::{Component, Path, PathBuf, StripPrefixError},
};

use crate::recipe::parser::Source;

use fs_err as fs;
use fs_err::File;
use zip::{result::ZipResult, ZipArchive};

pub mod copy_dir;
pub mod git_source;
pub mod patch;
pub mod url_source;

#[allow(missing_docs)]
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to download source from url: {0}")]
    Url(#[from] reqwest::Error),

    #[error("Url does not point to a file: {0}")]
    UrlNotFile(url::Url),

    #[error("WalkDir Error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("FileSystem error: '{0}'")]
    FileSystemError(fs_extra::error::Error),

    #[error("StripPrefixError Error: {0}")]
    StripPrefixError(#[from] StripPrefixError),

    #[error("Download could not be validated with checksum!")]
    ValidationFailed,

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Could not find `patch` executable")]
    PatchNotFound,

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

    #[error("Failed to extract archive: {0}")]
    TarExtractionError(String),

    #[error("Failed to extract zip archive: {0}")]
    ZipExtractionError(String),

    #[error("Failed to read from zip: {0}")]
    InvalidZip(String),

    #[error("Failed to run git command: {0}")]
    GitError(String),

    #[error("Failed to run git command: {0}")]
    GitErrorStr(&'static str),

    #[error("{0}")]
    UnknownError(String),

    #[error("{0}")]
    UnknownErrorStr(&'static str),

    #[error("Could not walk dir")]
    IgnoreError(#[from] ignore::Error),

    #[error("Failed to parse glob pattern")]
    Glob(#[from] globset::Error),

    #[error("No checksum found for url: {0}")]
    NoChecksum(url::Url),
}

/// Fetches all sources in a list of sources and applies specified patches
pub async fn fetch_sources(
    sources: &[Source],
    work_dir: &Path,
    recipe_dir: &Path,
    cache_dir: &Path,
) -> Result<(), SourceError> {
    let cache_src = cache_dir.join("src_cache");
    fs::create_dir_all(&cache_src)?;

    for src in sources {
        match &src {
            Source::Git(src) => {
                tracing::info!("Fetching source from git repo: {}", src.url());
                let result = git_source::git_src(src, &cache_src, recipe_dir)?;
                let dest_dir = if let Some(folder) = src.folder() {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                crate::source::copy_dir::CopyDir::new(&result, &dest_dir)
                    .use_gitignore(false)
                    .run()?;
                if !src.patches().is_empty() {
                    patch::apply_patches(src.patches(), work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url());

                let file_name_from_url = src
                    .url()
                    .path_segments()
                    .and_then(|segments| segments.last().map(|last| last.to_string()))
                    .ok_or_else(|| SourceError::UrlNotFile(src.url().clone()))?;

                let res = url_source::url_src(src, &cache_src).await?;
                let mut dest_dir = if let Some(folder) = src.folder() {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };

                // Create folder if it doesn't exist
                if !dest_dir.exists() {
                    fs::create_dir_all(&dest_dir)?;
                }

                if res
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .contains(".tar")
                {
                    extract(&res, &dest_dir, 1)?;
                    tracing::info!("Extracted to {:?}", dest_dir);
                } else if res
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .ends_with(".zip")
                {
                    extract_zip(&res, &dest_dir, 1)?;
                    tracing::info!("Extracted zip to {:?}", dest_dir);
                } else {
                    if let Some(file_name) = src.file_name() {
                        dest_dir = dest_dir.join(file_name);
                    } else {
                        dest_dir = dest_dir.join(file_name_from_url);
                    }
                    fs::copy(&res, &dest_dir)?;
                    tracing::info!("Downloaded to {:?}", dest_dir);
                }

                if !src.patches().is_empty() {
                    patch::apply_patches(src.patches(), work_dir, recipe_dir)?;
                }
            }
            Source::Path(src) => {
                let src_path = recipe_dir.join(src.path()).canonicalize()?;

                let dest_dir = if let Some(folder) = src.folder() {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };

                // Create folder if it doesn't exist
                if !dest_dir.exists() {
                    fs::create_dir_all(&dest_dir)?;
                }

                if !src_path.exists() {
                    return Err(SourceError::FileNotFound(src_path));
                }

                // check if the source path is a directory
                if src_path.is_dir() {
                    copy_dir::CopyDir::new(&src_path, &dest_dir)
                        .use_gitignore(src.use_gitignore())
                        .run()?;
                } else if let Some(file_name) = src
                    .file_name()
                    .cloned()
                    .or_else(|| src_path.file_name().map(PathBuf::from))
                {
                    tracing::info!(
                        "Copying source from path: {:?} to {:?}",
                        src_path,
                        dest_dir.join(&file_name)
                    );
                    fs::copy(&src_path, &dest_dir.join(file_name))?;
                } else {
                    return Err(SourceError::FileNotFound(src_path));
                }

                if !src.patches().is_empty() {
                    patch::apply_patches(src.patches(), work_dir, recipe_dir)?;
                }
            }
        }
    }
    Ok(())
}

/// Handle Compression formats internally
enum TarCompression<'a> {
    PlainTar(File),
    Gzip(flate2::read::GzDecoder<File>),
    Bzip2(bzip2::read::BzDecoder<File>),
    Xz2(xz2::read::XzDecoder<File>),
    Zstd(zstd::stream::read::Decoder<'a, std::io::BufReader<File>>),
    Compress,
    Lzip,
    Lzop,
}

fn ext_to_compression(ext: Option<&OsStr>, file: File) -> TarCompression {
    match ext
        .and_then(OsStr::to_str)
        .and_then(|s| s.rsplit_once('.'))
        .map(|(_, s)| s)
    {
        Some("gz" | "tgz" | "taz") => TarCompression::Gzip(flate2::read::GzDecoder::new(file)),
        Some("bz2" | "tbz" | "tbz2" | "tz2") => {
            TarCompression::Bzip2(bzip2::read::BzDecoder::new(file))
        }
        Some("lzma" | "tlz" | "xz" | "txz") => TarCompression::Xz2(xz2::read::XzDecoder::new(file)),
        Some("zst" | "tzst") => {
            TarCompression::Zstd(zstd::stream::read::Decoder::new(file).unwrap())
        }
        Some("Z" | "taZ") => TarCompression::Compress,
        Some("lz") => TarCompression::Lzip,
        Some("lzo") => TarCompression::Lzop,
        Some(_) | None => TarCompression::PlainTar(file),
    }
}

impl std::io::Read for TarCompression<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            TarCompression::PlainTar(reader) => reader.read(buf),
            TarCompression::Gzip(reader) => reader.read(buf),
            TarCompression::Bzip2(reader) => reader.read(buf),
            TarCompression::Xz2(reader) => reader.read(buf),
            TarCompression::Zstd(reader) => reader.read(buf),
            TarCompression::Compress | TarCompression::Lzip | TarCompression::Lzop => {
                todo!("unsupported for now")
            }
        }
    }
}

/// Extracts a tar archive to the specified target directory
fn extract(
    archive: &Path,
    target_directory: &Path,
    strip_components: usize,
) -> Result<(), SourceError> {
    let mut archive = tar::Archive::new(ext_to_compression(
        archive.file_name(),
        File::open(archive).map_err(|_| SourceError::FileNotFound(archive.to_path_buf()))?,
    ));

    for entry in archive.entries()? {
        let mut entry = entry?;
        let mut path = PathBuf::new();
        {
            // Essentially from https://github.com/alexcrichton/tar-rs/blob/34744459084c1fffb03d6c742f5a5af9a6403bc4/src/entry.rs#L381
            // for secure implementation of unpack, we skip all paths with ParentDir component as listed in below CVEs
            let entrypath = entry.path()?;
            for part in entrypath.components().skip(strip_components) {
                match part {
                    // Leading '/' characters, root paths, and '.'
                    // components are just ignored and treated as "empty
                    // components"
                    Component::Prefix(..) | Component::RootDir | Component::CurDir => continue,
                    // If any part of the filename is '..', then skip over
                    // unpacking the file to prevent directory traversal
                    // security issues.  See, e.g.: CVE-2001-1267,
                    // CVE-2002-0399, CVE-2005-1918, CVE-2007-4131
                    Component::ParentDir => continue,
                    Component::Normal(part) => path.push(part),
                }
            }
        }
        let path = target_directory.join(path);
        if entry.header().entry_type().is_dir() {
            // only errors if fails to create dir
            // and if file doesn't already exists
            std::fs::create_dir_all(&path)?;
            continue;
        }
        // create parent dir if doesn't already exists before unpacking
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        // should setup permissions and xattrs
        entry.unpack(path)?;
    }

    Ok(())
}

/// Extracts a zip archive to the specified target directory
/// currently this doesn't support bzip2 and zstd, zip archived with compression other than deflate would fail.
/// <!-- TODO: we can trivially add support for bzip2 and zstd by enabling the feature flags -->
fn extract_zip(
    archive: &Path,
    target_directory: &Path,
    strip_components: usize,
) -> Result<(), SourceError> {
    let archive = zip::ZipArchive::new(
        File::open(archive).map_err(|_| SourceError::FileNotFound(archive.to_path_buf()))?,
    )
    .map_err(|e| SourceError::InvalidZip(e.to_string()))?;

    extract_zip_stripped(archive, target_directory, strip_components)
        .map_err(|e| SourceError::ZipExtractionError(e.to_string()))?;

    Ok(())
}

fn extract_zip_stripped(
    mut zip: ZipArchive<File>,
    target_directory: &Path,
    strip_components: usize,
) -> ZipResult<()> {
    use std::fs;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let filepath = file
            .enclosed_name()
            .ok_or(zip::result::ZipError::InvalidArchive("Invalid file path"))?;

        let filepath = filepath
            .components()
            .skip(strip_components)
            .collect::<PathBuf>();
        if filepath.as_os_str().len() < 1 {
            continue;
        }
        let outpath = target_directory.join(filepath);

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
        // set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}
