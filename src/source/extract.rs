//! Helpers to extract archives
use std::{ffi::OsStr, io::BufRead, path::Path};

use crate::console_utils::LoggingOutputHandler;

use fs_err as fs;
use fs_err::File;

use super::SourceError;
/// Handle Compression formats internally
enum TarCompression<'a> {
    PlainTar(Box<dyn BufRead + 'a>),
    Gzip(flate2::read::GzDecoder<Box<dyn BufRead + 'a>>),
    Bzip2(bzip2::read::BzDecoder<Box<dyn BufRead + 'a>>),
    Xz2(xz2::read::XzDecoder<Box<dyn BufRead + 'a>>),
    Zstd(zstd::stream::read::Decoder<'a, std::io::BufReader<Box<dyn BufRead + 'a>>>),
    Compress,
    Lzip,
    Lzop,
}

/// Checks whether file has known tarball extension
pub fn is_tarball(file_name: &str) -> bool {
    [
        // Gzip
        ".tar.gz",
        ".tgz",
        ".taz",
        // Bzip2
        ".tar.bz2",
        ".tbz",
        ".tbz2",
        ".tz2",
        // Xz2
        ".tar.lzma",
        ".tlz",
        ".tar.xz",
        ".txz",
        // Zstd
        ".tar.zst",
        ".tzst",
        // Compress
        ".tar.Z",
        ".taZ",
        // Lzip
        ".tar.lz",
        // Lzop
        ".tar.lzo",
        // PlainTar
        ".tar",
    ]
    .iter()
    .any(|ext| file_name.ends_with(ext))
}

fn ext_to_compression<'a>(ext: Option<&OsStr>, file: Box<dyn BufRead + 'a>) -> TarCompression<'a> {
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
pub(crate) fn extract_tar(
    archive: impl AsRef<Path>,
    target_directory: impl AsRef<Path>,
    log_handler: &LoggingOutputHandler,
) -> Result<(), SourceError> {
    let archive = archive.as_ref();
    let target_directory = target_directory.as_ref();

    fs::create_dir_all(target_directory)?;

    let len = archive.metadata().map(|m| m.len()).unwrap_or(1);
    let progress_bar = log_handler.add_progress_bar(
        indicatif::ProgressBar::new(len)
            .with_prefix("Extracting tar")
            .with_style(log_handler.default_bytes_style()),
    );

    let file = File::open(archive)?;
    let buf_reader = std::io::BufReader::with_capacity(1024 * 1024, file);
    let wrapped = progress_bar.wrap_read(buf_reader);

    let mut archive = tar::Archive::new(ext_to_compression(archive.file_name(), Box::new(wrapped)));

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
pub(crate) fn extract_zip(
    archive: impl AsRef<Path>,
    target_directory: impl AsRef<Path>,
    log_handler: &LoggingOutputHandler,
) -> Result<(), SourceError> {
    let archive = archive.as_ref();
    let target_directory = target_directory.as_ref();
    fs::create_dir_all(target_directory)?;

    let len = archive.metadata().map(|m| m.len()).unwrap_or(1);
    let progress_bar = log_handler.add_progress_bar(
        indicatif::ProgressBar::new(len)
            .with_finish(indicatif::ProgressFinish::AndLeave)
            .with_prefix("Extracting zip")
            .with_style(log_handler.default_bytes_style()),
    );

    let file = File::open(archive)?;
    let buf_reader = std::io::BufReader::with_capacity(1024 * 1024, file);
    let wrapped = progress_bar.wrap_read(buf_reader);
    let mut archive =
        zip::ZipArchive::new(wrapped).map_err(|e| SourceError::InvalidZip(e.to_string()))?;

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
    use fs_err::{self as fs, File};
    use std::io::Write;

    use crate::{console_utils::LoggingOutputHandler, source::SourceError};

    use super::extract_zip;

    #[test]
    fn test_extract_zip() {
        // zip contains text.txt with "Hello, World" text
        const HELLO_WORLD_ZIP_FILE: &[u8] = &[
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
        _ = file.write_all(HELLO_WORLD_ZIP_FILE);

        let fancy_log = LoggingOutputHandler::default().with_multi_progress(multi_progress.clone());

        let res = extract_zip(file_path, tempdir.path(), &fancy_log);
        assert!(term.contents().trim().starts_with(
            "Extracting zip       [00:00:00] [━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━]"
        ));
        assert!(res.err().is_none());
        assert!(tempdir.path().join("text.txt").exists());
        assert!(
            fs::read_to_string(tempdir.path().join("text.txt"))
                .unwrap()
                .contains("Hello, World")
        );
    }

    #[test]
    fn test_extract_fail() {
        let fancy_log = LoggingOutputHandler::default();
        let tempdir = tempfile::tempdir().unwrap();
        let result = extract_zip("", tempdir.path(), &fancy_log);
        assert!(matches!(
            result,
            Err(SourceError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn test_extract_fail_2() {
        let fancy_log = LoggingOutputHandler::default();
        let tempdir = tempfile::tempdir().unwrap();
        let file = tempdir.path().join("test.zip");
        _ = File::create(&file);
        let res = extract_zip(file, tempdir.path(), &fancy_log);
        assert!(matches!(res.err(), Some(SourceError::InvalidZip(_))));
    }
}
