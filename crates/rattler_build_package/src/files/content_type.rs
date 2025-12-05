//! Content type detection for files

use std::fs::File;
use std::io::Read;
use std::path::Path;

use content_inspector::ContentType;

use crate::Result;

/// Detect the content type of a file
///
/// Reads the first 1024 bytes to determine if a file is text or binary,
/// and what encoding it uses.
pub fn detect_content_type(path: &Path) -> Result<ContentType> {
    if path.is_dir() || path.is_symlink() {
        // Directories and symlinks don't have a content type
        return Ok(ContentType::BINARY);
    }

    let mut file = File::open(path)?;
    let mut buffer = [0u8; 1024];
    let bytes_read = file.read(&mut buffer)?;

    let content = &buffer[..bytes_read];
    Ok(content_inspector::inspect(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_text_file() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Hello, world!")?;

        let ct = detect_content_type(&file_path)?;
        assert!(ct.is_text());

        Ok(())
    }

    #[test]
    fn test_binary_file() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test.bin");
        fs::write(&file_path, &[0u8, 1, 2, 3, 255])?;

        let ct = detect_content_type(&file_path)?;
        assert!(matches!(ct, ContentType::BINARY));

        Ok(())
    }
}
