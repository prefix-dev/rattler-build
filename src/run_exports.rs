use std::sync::Arc;

use futures::future::OptionFuture;
use rattler_cache::package_cache::{CacheKey, PackageCache, PackageCacheError};
use rattler_conda_types::{
    RepoDataRecord,
    package::{PackageFile, RunExportsJson},
};
use rattler_networking::retry_policies::default_retry_policy;
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
    package_cache: Option<(PackageCache, PackageCacheReporter)>,
    client: Option<ClientWithMiddleware>,
}

#[derive(Debug, Error)]
pub enum RunExportExtractorError {
    #[error(transparent)]
    PackageCache(#[from] PackageCacheError),

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

    /// Set the package cache that the extractor can use as well as a reporter
    /// to allow progress reporting.
    pub fn with_package_cache(
        self,
        package_cache: PackageCache,
        reporter: PackageCacheReporter,
    ) -> Self {
        Self {
            package_cache: Some((package_cache, reporter)),
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
        let Some((package_cache, mut package_cache_reporter)) = self.package_cache.clone() else {
            return Ok(None);
        };
        let Some(client) = self.client.as_ref() else {
            return Ok(None);
        };

        let progress_reporter = package_cache_reporter.add(record);
        let cache_key = CacheKey::from(&record.package_record);
        let url = record.url.clone();
        let max_concurrent_requests = self.max_concurrent_requests.clone();

        let _permit = OptionFuture::from(max_concurrent_requests.map(Semaphore::acquire_owned))
            .await
            .transpose()
            .expect("semaphore error");

        match package_cache
            .get_or_fetch_from_url_with_retry(
                cache_key,
                url,
                client.clone(),
                default_retry_policy(),
                Some(Arc::new(progress_reporter)),
            )
            .await
        {
            Ok(package_dir) => Ok(RunExportsJson::from_package_directory(package_dir.path()).ok()),
            Err(e) => Err(e.into()),
        }
    }
}
