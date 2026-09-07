use std::{collections::HashMap, str::FromStr};

use jiff::Timestamp;
use pyo3::{PyResult, exceptions::PyValueError};
use rattler_conda_types::{ChannelUrl, PackageName};
use rattler_redaction::Redact;
use rattler_solve::ExcludeNewer;

/// Convert Python cutoff arguments without enabling filtering for an empty policy.
pub(crate) fn exclude_newer_policy(
    cutoff: Option<Timestamp>,
    packages: Option<HashMap<String, Option<Timestamp>>>,
    channels: Option<HashMap<String, Option<Timestamp>>>,
    include_unknown_timestamp: bool,
) -> PyResult<Option<ExcludeNewer>> {
    let packages = packages.unwrap_or_default();
    let channels = channels.unwrap_or_default();
    if cutoff.is_none() && packages.is_empty() && channels.is_empty() {
        return Ok(None);
    }

    let mut policy = ExcludeNewer::from_datetime(cutoff.unwrap_or(Timestamp::MAX))
        .with_include_unknown_timestamp(include_unknown_timestamp);
    for (name, cutoff) in packages {
        let package = PackageName::from_str(&name).map_err(|err| {
            PyValueError::new_err(format!(
                "invalid exclude_newer_package name {name:?}: {err}"
            ))
        })?;
        policy = policy.with_package_cutoff(package, cutoff.unwrap_or(Timestamp::MAX));
    }
    for (channel, cutoff) in channels {
        let url = url::Url::parse(&channel).map_err(|err| {
            PyValueError::new_err(format!("invalid exclude_newer_channel URL: {err}"))
        })?;
        if url.cannot_be_a_base() {
            return Err(PyValueError::new_err(
                "exclude_newer_channel keys must be absolute channel URLs",
            ));
        }
        // Repodata records use the normalized base URL with credentials
        // redacted. Match that key while leaving transport URLs untouched.
        let channel = ChannelUrl::from(url).url().clone().redact().to_string();
        policy = policy.with_channel_cutoff(channel, cutoff.unwrap_or(Timestamp::MAX));
    }
    Ok(Some(policy))
}
