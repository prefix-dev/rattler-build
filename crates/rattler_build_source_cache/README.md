# Rattler Build Source Cache

A unified source cache for rattler-build that handles Git repositories, URL downloads, and local paths with proper caching, extraction, and concurrent access control.

## Features

- **Unified Cache**: Single interface for Git, URL, and Path sources
- **Content-Addressable Storage**: Sources are indexed by content hash for deduplication
- **Concurrent Access Control**: File locking ensures safe concurrent access from multiple processes
- **Automatic Extraction**: Archives (.tar.gz, .zip, .7z, etc.) are automatically extracted
- **Cache Management**: Automatic cleanup of old entries, configurable TTL
- **Progress Tracking**: Optional progress callbacks for downloads and extractions
- **Checksum Validation**: SHA256 and MD5 checksum support for URL downloads

## Usage

```rust
use rattler_build_source_cache::{SourceCacheBuilder, Source, UrlSource, GitSource, GitReference, Checksum};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a cache with custom configuration
    let cache = SourceCacheBuilder::new()
        .cache_dir("/path/to/cache")
        .max_age(chrono::Duration::days(30))
        .enable_cleanup(true)
        .cleanup_interval(std::time::Duration::from_secs(3600))
        .build()
        .await?;
    
    // Fetch a URL source
    let url_source = Source::Url(UrlSource {
        urls: vec!["https://example.com/archive.tar.gz".parse()?],
        checksum: Some(Checksum::Sha256(vec![/* hash bytes */])),
        file_name: None, // Will auto-extract
    });
    
    let path = cache.get_source(&url_source).await?;
    println!("Source cached at: {}", path.display());
    
    // Fetch a Git repository
    use rattler_build_source_cache::GitReference;
    
    let git_source = Source::Git(GitSource::new(
        "https://github.com/example/repo".parse()?,
        GitReference::Branch("main".to_string()),
        Some(1),
        false,
    ));
    
    let repo_path = cache.get_source(&git_source).await?;
    println!("Repository cloned at: {}", repo_path.display());
    
    // Path sources are passed through without caching
    let path_source = Source::Path("/local/path".into());
    let local_path = cache.get_source(&path_source).await?;
    assert_eq!(local_path, std::path::Path::new("/local/path"));
    
    Ok(())
}
```

## Cache Structure

The cache directory is organized as follows:

```
cache_dir/
├── .metadata/          # JSON metadata files for each cache entry
│   ├── <hash>.json
│   └── ...
├── .locks/            # Lock files for concurrent access control
│   ├── <hash>.lock
│   └── ...
├── <hash>_file.tar.gz # Downloaded archives
├── <hash>_extracted/  # Extracted archive contents
└── git_<hash>/        # Cloned git repositories
```

## Configuration

The `SourceCacheBuilder` provides various configuration options:

- `cache_dir`: Location of the cache directory (defaults to system cache dir)
- `client`: Custom HTTP client for downloads
- `max_age`: Maximum age before entries are considered stale
- `enable_cleanup`: Enable automatic cleanup of old entries
- `cleanup_interval`: How often to run cleanup
- `enable_compression`: Whether to compress cached files (future feature)
- `max_concurrent_downloads`: Limit concurrent downloads
- `progress_handler`: Custom progress reporting

## Thread Safety

The cache is designed for safe concurrent access:

- File locks prevent multiple processes from modifying the same cache entry
- The index uses async RwLock for thread-safe in-memory access
- Lock files are automatically cleaned up when guards are dropped

## License

BSD-3-Clause