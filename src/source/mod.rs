//! Module for fetching sources and applying patches

use std::{
    ffi::OsStr,
    path::{Path, PathBuf, StripPrefixError},
};

use crate::{
    metadata::Directories,
    recipe::parser::{GitRev, GitSource, Source},
    render::solver::default_bytes_style,
    tool_configuration,
};

use fs_err as fs;
use fs_err::File;

use crate::system_tools::SystemTools;
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
    directories: &Directories,
    system_tools: &SystemTools,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<Vec<Source>, SourceError> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }

    // Figure out the directories we need
    let work_dir = &directories.work_dir;
    let recipe_dir = &directories.recipe_dir;
    let cache_src = directories.output_dir.join("src_cache");
    fs::create_dir_all(&cache_src)?;

    let mut rendered_sources = Vec::new();

    for src in sources {
        match &src {
            Source::Git(src) => {
                tracing::info!("Fetching source from git repo: {}", src.url());
                let result = git_source::git_src(system_tools, src, &cache_src, recipe_dir)?;
                let dest_dir = if let Some(target_directory) = src.target_directory() {
                    work_dir.join(target_directory)
                } else {
                    work_dir.to_path_buf()
                };

                rendered_sources.push(Source::Git(GitSource {
                    rev: GitRev::Commit(result.1),
                    ..src.clone()
                }));

                crate::source::copy_dir::CopyDir::new(&result.0, &dest_dir)
                    .use_gitignore(false)
                    .run()?;
                if !src.patches().is_empty() {
                    patch::apply_patches(system_tools, src.patches(), work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url());

                let file_name_from_url = src
                    .url()
                    .path_segments()
                    .and_then(|segments| segments.last().map(|last| last.to_string()))
                    .ok_or_else(|| SourceError::UrlNotFile(src.url().clone()))?;

                let res = url_source::url_src(src, &cache_src, tool_configuration).await?;
                let mut dest_dir = if let Some(target_directory) = src.target_directory() {
                    work_dir.join(target_directory)
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
                    extract_tar(
                        &res,
                        &dest_dir,
                        tool_configuration.multi_progress_indicator.clone(),
                    )?;
                    tracing::info!("Extracted to {:?}", dest_dir);
                } else if res
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .ends_with(".zip")
                {
                    extract_zip(
                        &res,
                        &dest_dir,
                        tool_configuration.multi_progress_indicator.clone(),
                    )?;
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
                    patch::apply_patches(system_tools, src.patches(), work_dir, recipe_dir)?;
                }

                rendered_sources.push(Source::Url(src.clone()));
            }
            Source::Path(src) => {
                let src_path = recipe_dir.join(src.path()).canonicalize()?;

                let dest_dir = if let Some(target_directory) = src.target_directory() {
                    work_dir.join(target_directory)
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
                    patch::apply_patches(system_tools, src.patches(), work_dir, recipe_dir)?;
                }

                rendered_sources.push(Source::Path(src.clone()));
            }
        }
    }
    Ok(rendered_sources)
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

/// Moves the directory content from src to dest after stripping root dir, if present.
fn move_extracted_dir(src: &Path, dest: &Path) -> Result<(), SourceError> {
    let mut entries = fs::read_dir(src)?;
    let src_dir = match entries.next().transpose()? {
        // ensure if only single directory in entries(root dir)
        Some(dir) if entries.next().is_none() && dir.file_type()?.is_dir() => {
            src.join(dir.file_name())
        }
        _ => src.to_path_buf(),
    };

    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let destination = dest.join(entry.file_name());
        fs::rename(entry.path(), destination)?;
    }

    Ok(())
}

/// Extracts a tar archive to the specified target directory
fn extract_tar(
    archive: impl AsRef<Path>,
    target_directory: impl AsRef<Path>,
    multi_progress_indicator: indicatif::MultiProgress,
) -> Result<(), SourceError> {
    let archive = archive.as_ref();
    let target_directory = target_directory.as_ref();

    let len = archive.metadata().map(|m| m.len()).unwrap_or(1);
    let progress_bar = multi_progress_indicator.add(
        indicatif::ProgressBar::new(len)
            .with_prefix("Extracting tar")
            .with_style(default_bytes_style().map_err(|_| {
                SourceError::UnknownError("Failed to get progress bar style".to_string())
            })?),
    );

    let mut archive = tar::Archive::new(progress_bar.wrap_read(ext_to_compression(
        archive.file_name(),
        File::open(archive).map_err(|_| SourceError::FileNotFound(archive.to_path_buf()))?,
    )));

    let tmp_extraction_dir = tempfile::Builder::new().tempdir_in(target_directory)?;
    archive
        .unpack(&tmp_extraction_dir)
        .map_err(|e| SourceError::TarExtractionError(e.to_string()))?;

    move_extracted_dir(tmp_extraction_dir.path(), target_directory)?;
    progress_bar.finish_with_message("Extracted...");

    Ok(())
}

/// Extracts a zip archive to the specified target directory
/// currently this doesn't support bzip2 and zstd.
///
/// `.zip` files archived with compression other than deflate would fail.
///
/// <!-- TODO: we can trivially add support for bzip2 and zstd by enabling the feature flags -->
fn extract_zip(
    archive: impl AsRef<Path>,
    target_directory: impl AsRef<Path>,
    multi_progress_indicator: indicatif::MultiProgress,
) -> Result<(), SourceError> {
    let archive = archive.as_ref();
    let target_directory = target_directory.as_ref();

    let len = archive.metadata().map(|m| m.len()).unwrap_or(1);
    let progress_bar = multi_progress_indicator.add(
        indicatif::ProgressBar::new(len)
            .with_finish(indicatif::ProgressFinish::AndLeave)
            .with_prefix("Extracting zip")
            .with_style(default_bytes_style().map_err(|_| {
                SourceError::UnknownError("Failed to get progress bar style".to_string())
            })?),
    );

    let mut archive = zip::ZipArchive::new(progress_bar.wrap_read(
        File::open(archive).map_err(|_| SourceError::FileNotFound(archive.to_path_buf()))?,
    ))
    .map_err(|e| SourceError::InvalidZip(e.to_string()))?;

    let tmp_extraction_dir = tempfile::Builder::new().tempdir_in(target_directory)?;
    archive
        .extract(&tmp_extraction_dir)
        .map_err(|e| SourceError::ZipExtractionError(e.to_string()))?;

    move_extracted_dir(tmp_extraction_dir.path(), target_directory)?;
    progress_bar.finish_with_message("Extracted...");

    Ok(())
}

#[cfg(test)]
mod test {
    use std::{fs::File, io::Write};

    use crate::source::SourceError;

    use super::extract_zip;

    #[test]
    fn test_extract_zip() {
        // zip contains text.txt with "Hello, World" text
        const HELLOW_ZIP_FILE: &[u8] = &[
            80, 75, 3, 4, 10, 0, 0, 0, 0, 0, 244, 123, 36, 88, 144, 58, 246, 64, 13, 0, 0, 0, 13,
            0, 0, 0, 8, 0, 28, 0, 116, 101, 120, 116, 46, 116, 120, 116, 85, 84, 9, 0, 3, 4, 130,
            150, 101, 6, 130, 150, 101, 117, 120, 11, 0, 1, 4, 245, 1, 0, 0, 4, 20, 0, 0, 0, 72,
            101, 108, 108, 111, 44, 32, 87, 111, 114, 108, 100, 10, 80, 75, 1, 2, 30, 3, 10, 0, 0,
            0, 0, 0, 244, 123, 36, 88, 144, 58, 246, 64, 13, 0, 0, 0, 13, 0, 0, 0, 8, 0, 24, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 164, 129, 0, 0, 0, 0, 116, 101, 120, 116, 46, 116, 120, 116, 85,
            84, 5, 0, 3, 4, 130, 150, 101, 117, 120, 11, 0, 1, 4, 245, 1, 0, 0, 4, 20, 0, 0, 0, 80,
            75, 5, 6, 0, 0, 0, 0, 1, 0, 1, 0, 78, 0, 0, 0, 79, 0, 0, 0, 0, 0,
        ];
        let term = indicatif::InMemoryTerm::new(100, 100);
        let multi_progress = indicatif::MultiProgress::new();
        multi_progress.set_draw_target(indicatif::ProgressDrawTarget::term_like(Box::new(
            term.clone(),
        )));
        let tempdir = tempfile::tempdir().unwrap();
        let file_path = tempdir.path().join("test.zip");
        let mut file = File::create(&file_path).unwrap();
        _ = file.write_all(HELLOW_ZIP_FILE);

        let res = extract_zip(file_path, tempdir.path(), multi_progress.clone());
        assert!(term.contents().trim().starts_with(
            "Extracting zip       [00:00:00] [━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━]"
        ));
        assert!(matches!(res.err(), None));
        assert!(tempdir.path().join("text.txt").exists());
        assert!(std::fs::read_to_string(tempdir.path().join("text.txt"))
            .unwrap()
            .contains("Hello, World"));
    }

    #[test]
    fn test_extract_fail() {
        let multi_progress = indicatif::MultiProgress::new();
        let tempdir = tempfile::tempdir().unwrap();
        let res = extract_zip("", tempdir.path(), multi_progress.clone());
        assert!(matches!(res.err(), Some(SourceError::FileNotFound(_))));
    }

    #[test]
    fn test_extract_fail_2() {
        let multi_progress = indicatif::MultiProgress::new();
        let tempdir = tempfile::tempdir().unwrap();
        let file = tempdir.path().join("test.zip");
        _ = File::create(&file);
        let res = extract_zip(file, tempdir.path(), multi_progress.clone());
        assert!(matches!(res.err(), Some(SourceError::InvalidZip(_))));
    }
}
