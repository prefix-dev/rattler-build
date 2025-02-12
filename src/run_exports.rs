use std::sync::Arc;

use futures::future::OptionFuture;
use rattler::package_cache::CacheReporter;
use rattler_cache::archive_cache::{
    ArchiveCache, ArchiveCacheError, CacheKey, CacheKeyError as ArchiveCacheKeyError,
};
use rattler_conda_types::{package::RunExportsJson, RepoDataRecord};
use rattler_networking::retry_policies::default_retry_policy;
use rattler_package_streaming::ExtractError;
use reqwest_middleware::ClientWithMiddleware;
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::package_cache_reporter::PackageCacheReporter;

/// An object that can help extract run export information from a package.
///
/// This object can be configured with multiple sources and it will do its best
/// to find the run exports as fast as possible using the available resources.
#[derive(Default)]
pub struct RunExportExtractor {
    max_concurrent_requests: Option<Arc<Semaphore>>,
    archive_cache: Option<ArchiveCache>,
    reporter: Option<PackageCacheReporter>,
    client: Option<ClientWithMiddleware>,
}

#[derive(Debug, Error)]
pub enum RunExportExtractorError {
    #[error(transparent)]
    ArchiveCache(#[from] ArchiveCacheError),

    #[error(transparent)]
    Extract(#[from] ExtractError),

    #[error(transparent)]
    CacheKey(#[from] ArchiveCacheKeyError),

    #[error("the operation was cancelled")]
    Cancelled,
}

impl RunExportExtractor {
    /// Sets the maximum number of concurrent requests that the extractor can
    /// make.
    pub fn with_max_concurrent_requests(self, max_concurrent_requests: Arc<Semaphore>) -> Self {
        Self {
            max_concurrent_requests: Some(max_concurrent_requests),
            ..self
        }
    }

    /// Set the archive cache that the extractor can use
    pub fn with_archive_cache(self, archive_cache: ArchiveCache) -> Self {
        Self {
            archive_cache: Some(archive_cache),
            ..self
        }
    }

    /// Set the archive cache reporter
    pub fn with_reporter(self, reporter: PackageCacheReporter) -> Self {
        Self {
            reporter: Some(reporter),
            ..self
        }
    }

    /// Sets the download client that the extractor can use.
    pub fn with_client(self, client: ClientWithMiddleware) -> Self {
        Self {
            client: Some(client),
            ..self
        }
    }

    /// Extracts the run exports from a package. Returns `None` if no run
    /// exports are found.
    pub async fn extract(
        mut self,
        record: &RepoDataRecord,
    ) -> Result<Option<RunExportsJson>, RunExportExtractorError> {
        self.extract_into_package_cache(record).await
    }

    /// Extract the run exports from a package by downloading it to the cache
    /// and then reading the run_exports.json file.
    async fn extract_into_package_cache(
        &mut self,
        record: &RepoDataRecord,
    ) -> Result<Option<RunExportsJson>, RunExportExtractorError> {
        let Some(archive_cache) = self.archive_cache.clone() else {
            return Ok(None);
        };
        let Some(client) = self.client.clone() else {
            return Ok(None);
        };

        let reporter: Option<Arc<dyn CacheReporter>> =
            if let Some(mut reporter) = self.reporter.clone() {
                let progress_reporter = reporter.add(record);
                Some(Arc::new(progress_reporter))
            } else {
                None
            };

        // let progress_reporter = package_cache_reporter.add(record);
        let cache_key = CacheKey::new(&record.package_record, &record.file_name)?;
        let url = record.url.clone();
        let max_concurrent_requests = self.max_concurrent_requests.clone();

        let _permit = OptionFuture::from(max_concurrent_requests.map(Semaphore::acquire_owned))
            .await
            .transpose()
            .expect("semaphore error");

        match archive_cache
            .get_or_fetch_from_url_with_retry(
                &cache_key,
                url,
                client,
                default_retry_policy(),
                reporter,
            )
            .await
        {
            Ok(archive) => {
                let file =
                    rattler_package_streaming::seek::read_package_file::<RunExportsJson>(archive)?;
                Ok(Some(file))
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler::default_cache_dir;
    use rattler_conda_types::{PackageName, PackageRecord, Version};
    use std::str::FromStr;
    use url::Url;

    #[tokio::test]
    async fn test_extracting_run_exports_from_archive() {
        let archive_dir = default_cache_dir().unwrap().join("archive");

        let archive_cache = ArchiveCache::new(archive_dir);

        let run_exports_extractor = RunExportExtractor::default()
            .with_archive_cache(archive_cache)
            .with_client(ClientWithMiddleware::from(reqwest::Client::new()));

        let record = RepoDataRecord {
            package_record: PackageRecord::new(
                PackageName::from_str("zlib").unwrap(),
                Version::from_str("1.3.1").unwrap(),
                "hb9d3cd8_2".to_string(),
            ),
            url: Url::parse(
                "https://repo.prefix.dev/conda-forge/linux-64/zlib-1.3.1-hb9d3cd8_2.conda",
            )
            .unwrap(),
            file_name: "zlib-1.3.1-hb9d3cd8_2.conda".to_string(),
            channel: Some("conda-forge".to_string()),
        };

        let run_exports = run_exports_extractor
            .extract(&record)
            .await
            .unwrap()
            .unwrap();

        insta::assert_yaml_snapshot!(run_exports);
    }
}
