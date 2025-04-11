use std::sync::Arc;

use futures::future::OptionFuture;
use rattler_cache::run_exports_cache::{
    CacheKey, CacheKeyError, RunExportsCache, RunExportsCacheError,
};
use rattler_conda_types::{package::RunExportsJson, RepoDataRecord};
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
    run_exports_cache: Option<(RunExportsCache, PackageCacheReporter)>,
    client: Option<ClientWithMiddleware>,
}

#[derive(Debug, Error)]
pub enum RunExportExtractorError {
    #[error(transparent)]
    RunExportsCache(#[from] RunExportsCacheError),

    #[error(transparent)]
    CacheKey(#[from] CacheKeyError),

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

    /// Set the run exports cache that the extractor can use as well as a reporter
    /// to allow progress reporting.
    pub fn with_run_exports_cache(
        self,
        run_exports_cache: RunExportsCache,
        reporter: PackageCacheReporter,
    ) -> Self {
        Self {
            run_exports_cache: Some((run_exports_cache, reporter)),
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
        self.extract_into_run_exports_cache(record).await
    }

    /// Extract the run exports from a package by downloading and extracting only the run_exports.json file into the cache.
    async fn extract_into_run_exports_cache(
        &mut self,
        record: &RepoDataRecord,
    ) -> Result<Option<RunExportsJson>, RunExportExtractorError> {
        let Some((run_exports_cache, mut run_exports_cache_reporter)) =
            self.run_exports_cache.clone()
        else {
            return Ok(None);
        };
        let Some(client) = self.client.clone() else {
            return Ok(None);
        };

        let progress_reporter = run_exports_cache_reporter.add(record);
        let cache_key = CacheKey::create(&record.package_record, &record.file_name)?;
        let url = record.url.clone();
        let max_concurrent_requests = self.max_concurrent_requests.clone();

        let _permit = OptionFuture::from(max_concurrent_requests.map(Semaphore::acquire_owned))
            .await
            .transpose()
            .expect("semaphore error");

        match run_exports_cache
            .get_or_fetch_from_url_with_retry(
                &cache_key,
                url,
                client,
                default_retry_policy(),
                Some(Arc::new(progress_reporter)),
            )
            .await
        {
            Ok(cached_run_exports) => Ok(cached_run_exports.run_exports()),
            Err(e) => Err(e.into()),
        }
    }
}
