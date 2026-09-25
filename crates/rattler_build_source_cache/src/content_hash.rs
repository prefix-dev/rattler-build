//! Content hashing for extracted source archives.
//!
//! Implements the same algorithm as conda-build's `compute_content_hash`,
//! ensuring cross-tool hash compatibility.
//!
//! ## Algorithm
//!
//! 1. Walk the directory recursively **without** following symlinks.
//! 2. Sort all entries lexicographically by their full path string (backslashes
//!    normalised to forward slashes before comparing).
//! 3. For every entry, feed the following bytes into a running SHA-256 hasher:
//!    - The **relative** path with backslashes normalised to forward slashes (UTF-8).
//!    - A one-byte type tag: `b'L'` (symlink), `b'D'` (directory), `b'F'` (regular file).
//!    - The type payload (see below).
//!    - The separator byte `b'-'`.
//!
//!    **Payloads:**
//!    - Symlink -> the symlink target, with backslashes normalised to forward slashes (UTF-8).
//!    - Directory -> *(nothing)*.
//!    - Regular file -> the file contents with `\r\n` normalised to `\n` if the file is valid
//!      UTF-8; otherwise the raw bytes are fed unchanged.
//!
//! Returns the lower-case hex-encoded SHA-256 digest.

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Compute a SHA-256 content hash over an extracted directory tree.
pub fn compute_content_hash(dir: &Path) -> Result<String, io::Error> {
    // 1. Collect all entries without following symlinks.
    let mut entries: Vec<_> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;

    // 2. Sort lexicographically by the normalised full-path string,
    //    matching conda-build's `sorted(paths, key=str)` behaviour.
    entries.sort_by(|a, b| {
        let a_str = a.path().to_string_lossy().replace('\\', "/");
        let b_str = b.path().to_string_lossy().replace('\\', "/");
        a_str.cmp(&b_str)
    });

    let mut hasher = Sha256::new();

    // 3. Hash each entry in order.
    for entry in &entries {
        let path = entry.path();
        let file_type = entry.file_type();

        // Relative path, with backslashes normalised to forward slashes.
        let rel = path.strip_prefix(dir).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        hasher.update(rel_str.as_bytes());

        if file_type.is_symlink() {
            hasher.update(b"L");
            let target = fs_err::read_link(path)?;
            let target_str = target.to_string_lossy().replace('\\', "/");
            hasher.update(target_str.as_bytes());
        } else if file_type.is_dir() {
            hasher.update(b"D");
            // No payload for directories.
        } else if file_type.is_file() {
            hasher.update(b"F");
            // Normalise line endings for text files; use raw bytes for binary.
            let bytes = fs_err::read(path)?;
            if let Ok(text) = std::str::from_utf8(&bytes) {
                let normalised = text.replace("\r\n", "\n");
                hasher.update(normalised.as_bytes());
            } else {
                hasher.update(&bytes);
            }
        } else {
            return Err(io::Error::other(format!(
                "unsupported file type at path: {}",
                path.display()
            )));
        }

        // Separator byte prevents ambiguous concatenation.
        hasher.update(b"-");
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_err as fs;

    #[test]
    fn test_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let hash = compute_content_hash(tmp.path()).unwrap();
        // An empty directory produces the hash of no input bytes.
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_deterministic_across_calls() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), b"hello\n").unwrap();
        fs::write(tmp.path().join("b.txt"), b"world\n").unwrap();

        let h1 = compute_content_hash(tmp.path()).unwrap();
        let h2 = compute_content_hash(tmp.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_crlf_and_lf_are_equal() {
        let tmp_lf = tempfile::tempdir().unwrap();
        let tmp_crlf = tempfile::tempdir().unwrap();

        fs::write(tmp_lf.path().join("file.txt"), b"line1\nline2\n").unwrap();
        fs::write(tmp_crlf.path().join("file.txt"), b"line1\r\nline2\r\n").unwrap();

        let h_lf = compute_content_hash(tmp_lf.path()).unwrap();
        let h_crlf = compute_content_hash(tmp_crlf.path()).unwrap();
        assert_eq!(
            h_lf, h_crlf,
            "LF and CRLF files should produce the same content hash"
        );
    }

    #[test]
    fn test_different_content_differs() {
        let tmp1 = tempfile::tempdir().unwrap();
        let tmp2 = tempfile::tempdir().unwrap();

        fs::write(tmp1.path().join("file.txt"), b"hello").unwrap();
        fs::write(tmp2.path().join("file.txt"), b"world").unwrap();

        let h1 = compute_content_hash(tmp1.path()).unwrap();
        let h2 = compute_content_hash(tmp2.path()).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_binary_file_not_crlf_normalised() {
        let tmp_crlf = tempfile::tempdir().unwrap();
        let tmp_lf = tempfile::tempdir().unwrap();

        // These bytes are not valid UTF-8, so the file is treated as binary.
        // CRLF bytes within binary content must NOT be normalised.
        fs::write(tmp_crlf.path().join("data.bin"), b"\xFF\xFE\r\n\x00").unwrap();
        fs::write(tmp_lf.path().join("data.bin"), b"\xFF\xFE\n\x00").unwrap();

        let h_crlf = compute_content_hash(tmp_crlf.path()).unwrap();
        let h_lf = compute_content_hash(tmp_lf.path()).unwrap();

        assert_ne!(h_crlf, h_lf, "CRLF in binary files must not be normalised");
    }

    #[test]
    fn test_binary_file_known_hash() {
        let tmp = tempfile::tempdir().unwrap();

        // A binary file whose bytes are not valid UTF-8 - raw bytes are fed
        // into the hasher unchanged. The expected hash was independently
        // computed by feeding the algorithm inputs by hand:
        //   SHA256("data.bin" || "F" || <raw bytes> || "-")
        // where <raw bytes> = [0xFF, 0xFE, 0x00, 0x01, 0x02, 0x03].
        fs::write(
            tmp.path().join("data.bin"),
            [0xFF_u8, 0xFE, 0x00, 0x01, 0x02, 0x03],
        )
        .unwrap();

        let hash = compute_content_hash(tmp.path()).unwrap();
        assert_eq!(
            hash, "ef0aec9048aa0e1214615052f37404dfedc79cb6dd972b3ac12c88c3230fbc18",
            "binary file hash does not match known-correct value"
        );
    }

    #[test]
    fn test_hash_extracted_archive_with_mixed_content() {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let tmp = tempfile::tempdir().unwrap();
        let archive_path = tmp.path().join("test.tar.gz");

        // Build a tar.gz containing a directory, a text file (CRLF), a binary
        // file, and (on Unix) a symlink.
        {
            let gz = GzEncoder::new(
                fs::File::create(&archive_path).unwrap(),
                Compression::default(),
            );
            let mut builder = tar::Builder::new(gz);

            // Directory
            let mut dir_header = tar::Header::new_gnu();
            dir_header.set_entry_type(tar::EntryType::Directory);
            dir_header.set_path("subdir/").unwrap();
            dir_header.set_size(0);
            dir_header.set_mode(0o755);
            dir_header.set_cksum();
            builder.append(&dir_header, std::io::empty()).unwrap();

            // Text file with CRLF line endings
            let text: &[u8] = b"line1\r\nline2\r\n";
            let mut text_header = tar::Header::new_gnu();
            text_header.set_path("subdir/text.txt").unwrap();
            text_header.set_size(text.len() as u64);
            text_header.set_mode(0o644);
            text_header.set_cksum();
            builder.append(&text_header, text).unwrap();

            // Binary file (invalid UTF-8)
            let binary: &[u8] = &[0xFF, 0xFE, 0x00, 0x01, 0x02, 0x03];
            let mut bin_header = tar::Header::new_gnu();
            bin_header.set_path("data.bin").unwrap();
            bin_header.set_size(binary.len() as u64);
            bin_header.set_mode(0o644);
            bin_header.set_cksum();
            builder.append(&bin_header, binary).unwrap();

            // Symlink (Unix only - Windows symlinks require elevated privileges)
            #[cfg(unix)]
            {
                let mut sym_header = tar::Header::new_gnu();
                sym_header.set_entry_type(tar::EntryType::Symlink);
                sym_header.set_path("link.txt").unwrap();
                sym_header.set_link_name("subdir/text.txt").unwrap();
                sym_header.set_size(0);
                sym_header.set_mode(0o777);
                sym_header.set_cksum();
                builder.append(&sym_header, std::io::empty()).unwrap();
            }

            builder.into_inner().unwrap().finish().unwrap();
        }

        // Extract the archive.
        let extract_dir = tmp.path().join("extracted");
        fs::create_dir_all(&extract_dir).unwrap();
        {
            let file = fs::File::open(&archive_path).unwrap();
            let gz = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(&extract_dir).unwrap();
        }

        // Hash must be stable across two calls and be a valid SHA-256 hex string.
        let h1 = compute_content_hash(&extract_dir).unwrap();
        let h2 = compute_content_hash(&extract_dir).unwrap();
        assert_eq!(h1, h2, "content hash must be deterministic");
        assert_eq!(h1.len(), 64, "SHA-256 hex digest must be 64 characters");

        // On Unix the archive contains: data.bin (binary), link.txt (symlink ->
        // subdir/text.txt), subdir/ (directory), subdir/text.txt (text, CRLF
        // normalised to LF). Windows omits the symlink entry.
        #[cfg(unix)]
        assert_eq!(
            h1, "40e0c58218df2305912c025c160c4247825f3b4d23432a5df3abd6f2a558ee4b",
            "content hash does not match known-correct value"
        );
    }
}
