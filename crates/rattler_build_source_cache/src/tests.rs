//! Tests for the source cache

#[cfg(test)]
mod source_cache_tests {
    use super::super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_cache_builder() {
        let temp_dir = TempDir::new().unwrap();
        let _cache = SourceCacheBuilder::new()
            .cache_dir(temp_dir.path())
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
        let checksum = Checksum::Sha256(vec![1, 2, 3, 4]);

        let key1 = CacheIndex::generate_cache_key(&url, std::slice::from_ref(&checksum));
        let key2 = CacheIndex::generate_cache_key(&url, &[checksum]);

        assert_eq!(key1, key2);

        let key3 = CacheIndex::generate_cache_key(&url, &[]);
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
            attestation_verified: false,
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
        fs_err::write(&file_path, data).unwrap();

        // Validate should succeed
        assert!(checksum.validate(&file_path).is_ok());

        // Write different data
        fs_err::write(&file_path, b"different data").unwrap();

        // Validate should fail
        let err = checksum.validate(&file_path).unwrap_err();
        assert_ne!(err.expected, err.actual);
    }

    #[tokio::test]
    async fn test_path_source_passthrough() {
        let temp_dir = TempDir::new().unwrap();
        let cache = SourceCacheBuilder::new()
            .cache_dir(temp_dir.path())
            .build()
            .await
            .unwrap();

        let path = std::path::PathBuf::from("/some/local/path");
        let source = Source::Path(path.clone());

        let result = cache.get_source(&source).await.unwrap();
        assert_eq!(result.path, path);
        assert_eq!(result.git_commit, None);
    }

    #[tokio::test]
    async fn test_git_source_creation() {
        use crate::GitReference;

        let url = url::Url::parse("https://github.com/example/repo.git").unwrap();
        let reference = GitReference::Branch("main".to_string());

        let git_source = GitSource::new(url.clone(), reference.clone(), Some(1), false, true);

        assert_eq!(git_source.url, url);
        assert_eq!(git_source.reference, reference);
        assert_eq!(git_source.depth, Some(1));
        assert!(!git_source.lfs);
        assert!(git_source.submodules);

        let git_url = git_source.to_git_url();
        assert_eq!(git_url.repository(), &url);
        assert_eq!(git_url.reference(), &reference);
    }

    #[test]
    fn test_should_extract_with_actual_filename() {
        use crate::cache::is_archive;
        use std::path::Path;

        // Test the is_archive function directly
        assert!(
            is_archive("file.tar.gz"),
            "Should detect .tar.gz as archive"
        );
        assert!(
            is_archive("YODA-2.0.1.tar.gz"),
            "Should detect .tar.gz with version as archive"
        );
        assert!(is_archive("file.zip"), "Should detect .zip as archive");
        assert!(
            is_archive("file.tar.bz2"),
            "Should detect .tar.bz2 as archive"
        );

        // Test that non-archives are correctly identified
        assert!(!is_archive("file.txt"), "Should not detect .txt as archive");
        assert!(!is_archive("file.pdf"), "Should not detect .pdf as archive");
        assert!(
            !is_archive("no_extension"),
            "Should not detect files without extension as archive"
        );

        // Test the specific case from the issue: URL with query parameters
        // When the cache_path is hash-based but actual_filename from Content-Disposition is an archive
        let hash_path = Path::new("abcd1234efgh5678");
        assert!(
            !is_archive(hash_path.to_str().unwrap()),
            "Hash-based filename should not be detected as archive"
        );

        // But the actual filename from Content-Disposition should be detected
        let actual_filename = "YODA-2.0.1.tar.gz";
        assert!(
            is_archive(actual_filename),
            "Content-Disposition filename should be detected as archive"
        );
    }

    #[tokio::test]
    async fn test_should_extract_based_on_path() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let cache = SourceCacheBuilder::new()
            .cache_dir(temp_dir.path())
            .build()
            .await
            .unwrap();

        // Hash-based path (no archive extension) should not extract
        let hash_path = temp_dir.path().join("abcd1234efgh5678_download");
        assert!(
            !cache.should_extract(&hash_path),
            "Hash-based path should not be extracted"
        );

        // Path with archive extension should extract
        // (this is the case after download_url renames the file using Content-Disposition)
        let archive_path = temp_dir.path().join("abcd1234_YODA-2.0.1.tar.gz");
        assert!(
            cache.should_extract(&archive_path),
            "Path with archive extension should be extracted"
        );

        // Non-archive path should not extract
        let non_archive_path = temp_dir.path().join("abcd1234_file.txt");
        assert!(
            !cache.should_extract(&non_archive_path),
            "Non-archive path should not be extracted"
        );
    }

    /// Regression test for <https://github.com/prefix-dev/rattler-build/issues/2379>.
    ///
    /// Submodules that use relative URLs (e.g. `../sibling.git`) are resolved
    /// against the origin remote. Because rattler_git creates checkouts with
    /// `git clone --local`, origin points at the local bare cache database
    /// instead of the real remote. We resolve relative submodule URLs against
    /// the real source URL ourselves (similar to Cargo's approach) so that
    /// `git submodule update` uses the correct absolute URLs.
    #[tokio::test]
    async fn test_git_submodule_relative_url() {
        use crate::GitReference;
        use crate::source::{GitSource, Source};
        use std::path::Path;
        use std::process::Command;

        let tmp = TempDir::new().unwrap();
        let repos_dir = tmp.path().join("repos");
        fs_err::create_dir_all(&repos_dir).unwrap();

        // Helper: run git with a fake identity and file protocol allowed.
        let git = |dir: &Path, args: &[&str]| -> std::process::Output {
            let out = Command::new("git")
                .current_dir(dir)
                .arg("-c")
                .arg("user.name=test")
                .arg("-c")
                .arg("user.email=test@test")
                .arg("-c")
                .arg("protocol.file.allow=always")
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr),
            );
            out
        };

        // Create a "sub" repo that will serve as the submodule.
        let sub_repo = repos_dir.join("sub.git");
        git(&repos_dir, &["init", "--bare", sub_repo.to_str().unwrap()]);

        // Populate it via a temporary worktree.
        let sub_work = tmp.path().join("sub_work");
        git(
            &repos_dir,
            &[
                "clone",
                sub_repo.to_str().unwrap(),
                sub_work.to_str().unwrap(),
            ],
        );
        fs_err::write(sub_work.join("hello.txt"), "hello from submodule").unwrap();
        git(&sub_work, &["add", "."]);
        git(&sub_work, &["commit", "-m", "init sub"]);
        git(&sub_work, &["push"]);

        // Create a "main" bare repo that references "sub" via a relative URL.
        let main_repo = repos_dir.join("main.git");
        git(&repos_dir, &["init", "--bare", main_repo.to_str().unwrap()]);
        let main_work = tmp.path().join("main_work");
        git(
            &repos_dir,
            &[
                "clone",
                main_repo.to_str().unwrap(),
                main_work.to_str().unwrap(),
            ],
        );

        // Add the submodule using a relative URL ("../sub.git").
        git(&main_work, &["submodule", "add", "../sub.git", "ext/sub"]);

        fs_err::write(main_work.join("README"), "main repo").unwrap();
        git(&main_work, &["add", "."]);
        git(&main_work, &["commit", "-m", "init with submodule"]);
        git(&main_work, &["push"]);

        // Get the commit we just pushed, so we can use it as a tag-like ref.
        let out = git(&main_work, &["rev-parse", "HEAD"]);
        let commit = String::from_utf8(out.stdout).unwrap().trim().to_string();

        // Now exercise the source cache against the bare "main.git" repo.
        let cache_dir = tmp.path().join("cache");
        let url = url::Url::from_file_path(&main_repo).unwrap();

        let cache = SourceCacheBuilder::new()
            .cache_dir(&cache_dir)
            .build()
            .await
            .unwrap();

        let source = Source::Git(GitSource::with_expected_commit(
            url,
            GitReference::from_rev(commit.clone()),
            None,
            false,
            true,
            Some(commit),
        ));

        // This used to fail because git submodule update tried to resolve
        // "../sub.git" against the local cache bare database path.
        let result = cache.get_source(&source).await.unwrap();

        // Verify the submodule was actually checked out.
        let sub_file = result.path.join("ext/sub/hello.txt");
        assert!(
            sub_file.exists(),
            "submodule file should exist at {}",
            sub_file.display()
        );
        assert_eq!(
            fs_err::read_to_string(&sub_file).unwrap(),
            "hello from submodule"
        );
    }

    #[test]
    fn test_extract_filename_from_header_strips_path_components() {
        use crate::cache::extract_filename_from_header;

        // Simple filename
        assert_eq!(
            extract_filename_from_header("attachment; filename=\"file.tar.gz\""),
            Some("file.tar.gz".to_string())
        );

        // Filename with path components (e.g. from GitHub raw downloads)
        assert_eq!(
            extract_filename_from_header(
                "attachment; filename=\"testdata/highgui/video/VID00003-20100701-2204.avi\""
            ),
            Some("VID00003-20100701-2204.avi".to_string())
        );

        // Filename with single path component
        assert_eq!(
            extract_filename_from_header("attachment; filename=\"subdir/archive.tar.gz\""),
            Some("archive.tar.gz".to_string())
        );

        // Empty filename
        assert_eq!(
            extract_filename_from_header("attachment; filename=\"\""),
            None
        );
    }
}
