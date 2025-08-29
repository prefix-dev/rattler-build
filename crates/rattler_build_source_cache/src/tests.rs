//! Tests for the source cache

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::lock::LockManager;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_cache_builder() {
        let temp_dir = TempDir::new().unwrap();
        let _cache = SourceCacheBuilder::new()
            .cache_dir(temp_dir.path())
            .max_age(chrono::Duration::days(7))
            .enable_cleanup(false)
            .build()
            .await
            .unwrap();

        assert!(temp_dir.path().exists());
        assert!(temp_dir.path().join(".metadata").exists());
        assert!(temp_dir.path().join(".locks").exists());
    }

    #[tokio::test]
    async fn test_cache_key_generation() {
        let url = url::Url::parse("https://example.com/file.tar.gz").unwrap();
        let checksum = Some(Checksum::Sha256(vec![1, 2, 3, 4]));

        let key1 = CacheIndex::generate_cache_key(&url, checksum.as_ref());
        let key2 = CacheIndex::generate_cache_key(&url, checksum.as_ref());

        assert_eq!(key1, key2);

        let key3 = CacheIndex::generate_cache_key(&url, None);
        assert_ne!(key1, key3);
    }

    #[tokio::test]
    async fn test_git_cache_key_generation() {
        let url = "https://github.com/example/repo.git";
        let rev = "main";

        let key1 = CacheIndex::generate_git_cache_key(url, rev);
        let key2 = CacheIndex::generate_git_cache_key(url, rev);

        assert_eq!(key1, key2);
        assert!(key1.starts_with("git_"));

        let key3 = CacheIndex::generate_git_cache_key(url, "develop");
        assert_ne!(key1, key3);
    }

    #[tokio::test]
    async fn test_cache_entry_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let index = CacheIndex::new(temp_dir.path().to_path_buf())
            .await
            .unwrap();

        let entry = CacheEntry {
            source_type: SourceType::Url,
            url: "https://example.com/file.tar.gz".to_string(),
            checksum: Some("abcd1234".to_string()),
            checksum_type: Some("sha256".to_string()),
            actual_filename: Some("file.tar.gz".to_string()),
            git_commit: None,
            git_rev: None,
            cache_path: std::path::PathBuf::from("file_abcd.tar.gz"),
            extracted_path: Some(std::path::PathBuf::from("file_abcd_extracted")),
            last_accessed: chrono::Utc::now(),
            created: chrono::Utc::now(),
            lock_file: None,
        };

        let key = "test_key";
        index.insert(key.to_string(), entry.clone()).await.unwrap();

        // Retrieve the entry
        let retrieved = index.get(key).await.unwrap();
        assert_eq!(retrieved.url, entry.url);
        assert_eq!(retrieved.checksum, entry.checksum);
        assert_eq!(retrieved.actual_filename, entry.actual_filename);
    }

    #[tokio::test]
    async fn test_lock_manager() {
        let temp_dir = TempDir::new().unwrap();
        let lock_manager = LockManager::new(temp_dir.path()).await.unwrap();

        // Acquire a lock
        let guard1 = lock_manager.acquire("test_lock").await.unwrap();
        assert!(guard1.path().exists());

        // Try to acquire the same lock (should fail with try_acquire)
        let result = lock_manager.try_acquire("test_lock");
        assert!(result.is_err());

        // Drop the first guard
        drop(guard1);

        // Now we should be able to acquire it
        let guard2 = lock_manager.acquire("test_lock").await.unwrap();
        assert!(guard2.path().exists());
    }

    #[tokio::test]
    async fn test_checksum_validation() {
        use sha2::{Digest, Sha256};

        let data = b"test data";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize().to_vec();

        let checksum = Checksum::Sha256(hash.clone());

        // Create a temp file with the data
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_file");
        std::fs::write(&file_path, data).unwrap();

        // Validate should succeed
        assert!(checksum.validate(&file_path));

        // Write different data
        std::fs::write(&file_path, b"different data").unwrap();

        // Validate should fail
        assert!(!checksum.validate(&file_path));
    }

    #[tokio::test]
    async fn test_path_source_passthrough() {
        let temp_dir = TempDir::new().unwrap();
        let cache = SourceCacheBuilder::new()
            .cache_dir(temp_dir.path())
            .enable_cleanup(false)
            .build()
            .await
            .unwrap();

        let path = std::path::PathBuf::from("/some/local/path");
        let source = Source::Path(path.clone());

        let result = cache.get_source(&source).await.unwrap();
        assert_eq!(result, path);
    }

    #[tokio::test]
    async fn test_git_source_creation() {
        use crate::GitReference;

        let url = url::Url::parse("https://github.com/example/repo.git").unwrap();
        let reference = GitReference::Branch("main".to_string());

        let git_source = GitSource::new(url.clone(), reference.clone(), Some(1), false);

        assert_eq!(git_source.url, url);
        assert_eq!(git_source.reference, reference);
        assert_eq!(git_source.depth, Some(1));
        assert!(!git_source.lfs);

        let git_url = git_source.to_git_url();
        assert_eq!(git_url.repository(), &url);
        assert_eq!(git_url.reference(), &reference);
    }
}
