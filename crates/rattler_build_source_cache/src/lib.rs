//! # Rattler Build Source Cache
//!
//! This crate provides a unified source cache for rattler-build, handling Git repositories,
//! URL downloads, and local paths with proper caching, extraction, and concurrent access control.

pub mod builder;
pub mod cache;
pub mod error;
pub mod index;
pub mod lock;
pub mod source;

pub use builder::SourceCacheBuilder;
pub use cache::SourceCache;
pub use error::CacheError;
pub use index::{CacheEntry, CacheIndex, SourceType};
pub use rattler_build_networking::{BaseClient, BaseClientBuilder};
pub use rattler_git::GitUrl;
pub use rattler_git::git::GitReference;
pub use source::{Checksum, GitSource, Source, UrlSource};

#[cfg(test)]
mod tests;
