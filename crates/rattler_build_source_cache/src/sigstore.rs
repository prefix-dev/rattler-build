//! Sigstore attestation verification
//!
//! This module contains all sigstore-related functionality for verifying
//! attestation bundles against source artifacts. It handles standard Sigstore
//! bundles, PyPI PEP 740 provenance responses, and GitHub's attestations API.

use std::path::Path;

use sha2::{Digest, Sha256};
use sigstore_trust_root::TrustedRoot;
use sigstore_types::SignatureContent;
use sigstore_verify::{VerificationPolicy, verify};

use crate::error::CacheError;
use crate::source::AttestationVerification;
use rattler_build_networking::BaseClient;

/// Auto-derive a PyPI provenance URL from a PyPI source URL.
///
/// Detects URLs from `pypi.io` and `files.pythonhosted.org` and constructs
/// the corresponding `https://pypi.org/integrity/{project}/{version}/{filename}/provenance` URL.
fn derive_pypi_provenance_url(source_url: &url::Url) -> Option<url::Url> {
    let host = source_url.host_str()?;
    if host != "pypi.io" && host != "files.pythonhosted.org" {
        return None;
    }

    // PyPI URLs look like:
    // https://pypi.io/packages/source/f/flask/flask-3.1.1.tar.gz
    // https://files.pythonhosted.org/packages/source/f/flask/flask-3.1.1.tar.gz
    // or with hashes:
    // https://files.pythonhosted.org/packages/ab/cd/.../flask-3.1.1.tar.gz
    let path = source_url.path();
    let filename = path.rsplit('/').next()?;

    // Extract project name and version from the filename. Source distribution
    // filenames are project-version archives; wheel filenames are
    // project-version-python-abi-platform per PEP 427.
    let (project, version) = if let Some(stem) = filename.strip_suffix(".whl") {
        let mut parts = stem.splitn(3, '-');
        let project = parts.next()?;
        let version = parts.next()?;
        parts.next()?;
        (project, version)
    } else {
        let stem = filename
            .strip_suffix(".tar.gz")
            .or_else(|| filename.strip_suffix(".tar.bz2"))
            .or_else(|| filename.strip_suffix(".zip"))?;
        stem.rsplit_once('-')?
    };

    // Normalize project name (PEP 503: replace [-_.] with -)
    let normalized_project = project.to_lowercase().replace(['-', '_', '.'], "-");

    let provenance_url = format!(
        "https://pypi.org/integrity/{}/{}/{}/provenance",
        normalized_project, version, filename
    );
    url::Url::parse(&provenance_url).ok()
}

/// Derive the GitHub repository attestations API URL for a release asset.
///
/// Filtering by the downloaded artifact's digest is important: GitHub release
/// attestations also contain the commit SHA-1, but that does not authenticate
/// GitHub's dynamically generated source archives. An uploaded release asset,
/// on the other hand, is included as a SHA-256 subject and can be verified.
fn derive_github_attestations_url(
    source_url: &url::Url,
    artifact_sha256_hex: &str,
) -> Option<url::Url> {
    if source_url.host_str()? != "github.com" {
        return None;
    }

    let segments: Vec<_> = source_url.path_segments()?.collect();
    if segments.len() < 6 || segments[2] != "releases" || segments[3] != "download" {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1].strip_suffix(".git").unwrap_or(segments[1]);
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    // The digest is a path parameter in GitHub's public list-attestations API.
    // Keep the predicate filter as well so unrelated attestations for the same
    // artifact are not considered release attestations.
    let mut api_url = url::Url::parse(&format!(
        "https://api.github.com/repos/{owner}/{repo}/attestations/sha256:{artifact_sha256_hex}"
    ))
    .ok()?;
    api_url
        .query_pairs_mut()
        .append_pair("predicate_type", "release");
    Some(api_url)
}

/// Result of parsing an attestation response.
#[derive(Debug)]
struct ParsedAttestations {
    bundles: Vec<sigstore_types::Bundle>,
    /// Whether these bundles were converted from PyPI PEP 740 provenance format.
    /// PyPI-converted bundles lack canonicalized rekor bodies so transparency log
    /// verification must be skipped.
    from_pypi: bool,
    /// Whether these are GitHub immutable-release attestations. They use
    /// GitHub's Sigstore trust domain and RFC 3161 timestamps instead of Rekor.
    from_github: bool,
}

/// Parse an attestation response into one or more sigstore bundles.
///
/// Handles two formats:
/// 1. **Standard sigstore bundle** (`.sigstore.json`): has a `mediaType` field,
///    parsed directly via `Bundle::from_json`.
/// 2. **PyPI PEP 740 provenance response**: has an `attestation_bundles` array,
///    each containing `attestations` that are converted to sigstore bundles.
/// 3. **GitHub attestations API response**: has an `attestations` array whose
///    entries each contain a standard bundle in their `bundle` field.
fn parse_attestation_response(json_str: &str) -> Result<ParsedAttestations, CacheError> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| CacheError::InvalidAttestationBundle(format!("Invalid JSON: {}", e)))?;

    // If it has a "mediaType" field, it's a standard sigstore bundle
    if value.get("mediaType").is_some() {
        let bundle = sigstore_types::Bundle::from_json(json_str).map_err(|e| {
            CacheError::InvalidAttestationBundle(format!("Failed to parse sigstore bundle: {}", e))
        })?;
        return Ok(ParsedAttestations {
            bundles: vec![bundle],
            from_pypi: false,
            from_github: false,
        });
    }

    // GitHub's repository attestations API wraps each standard bundle.
    if let Some(attestations) = value.get("attestations").and_then(|v| v.as_array()) {
        let mut bundles = Vec::new();
        for attestation in attestations {
            let Some(bundle_value) = attestation.get("bundle") else {
                continue;
            };
            let bundle_json = serde_json::to_string(bundle_value).map_err(|e| {
                CacheError::InvalidAttestationBundle(format!(
                    "Failed to serialize GitHub attestation bundle: {e}"
                ))
            })?;
            let bundle = sigstore_types::Bundle::from_json(&bundle_json).map_err(|e| {
                CacheError::InvalidAttestationBundle(format!(
                    "Failed to parse GitHub attestation bundle: {e}"
                ))
            })?;
            bundles.push(bundle);
        }
        if bundles.is_empty() {
            return Err(CacheError::InvalidAttestationBundle(
                "GitHub returned no release attestation for the artifact SHA-256; dynamically generated source archives are not covered by GitHub release attestations, so upload the source archive as a release asset"
                    .to_string(),
            ));
        }
        return Ok(ParsedAttestations {
            bundles,
            from_pypi: false,
            from_github: true,
        });
    }

    // Otherwise, try to parse as a PyPI provenance response
    if let Some(attestation_bundles) = value.get("attestation_bundles").and_then(|v| v.as_array()) {
        let mut bundles = Vec::new();
        for ab in attestation_bundles {
            if let Some(attestations) = ab.get("attestations").and_then(|v| v.as_array()) {
                for attestation in attestations {
                    let bundle = convert_pypi_attestation_to_bundle(attestation)?;
                    bundles.push(bundle);
                }
            }
        }
        if bundles.is_empty() {
            return Err(CacheError::InvalidAttestationBundle(
                "PyPI provenance response contains no attestations".to_string(),
            ));
        }
        return Ok(ParsedAttestations {
            bundles,
            from_pypi: true,
            from_github: false,
        });
    }

    Err(CacheError::InvalidAttestationBundle(
        "Unrecognized attestation format: expected a Sigstore bundle, PyPI provenance response, or GitHub attestations API response"
            .to_string(),
    ))
}

/// Convert a PyPI PEP 740 attestation object to a sigstore v0.3 bundle.
///
/// PyPI attestation format:
/// ```json
/// {
///   "version": 1,
///   "verification_material": {
///     "certificate": "<base64(DER)>",
///     "transparency_entries": [{ ... }]
///   },
///   "envelope": {
///     "statement": "<base64(in-toto JSON)>",
///     "signature": "<base64(sig)>"
///   }
/// }
/// ```
fn convert_pypi_attestation_to_bundle(
    attestation: &serde_json::Value,
) -> Result<sigstore_types::Bundle, CacheError> {
    let err = |msg: &str| CacheError::InvalidAttestationBundle(msg.to_string());

    let envelope = attestation
        .get("envelope")
        .ok_or_else(|| err("missing 'envelope'"))?;
    let verification_material = attestation
        .get("verification_material")
        .ok_or_else(|| err("missing 'verification_material'"))?;

    let statement = envelope
        .get("statement")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("missing 'envelope.statement'"))?;
    let signature = envelope
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("missing 'envelope.signature'"))?;
    let certificate = verification_material
        .get("certificate")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("missing 'verification_material.certificate'"))?;

    // PyPI transparency_entries already use the sigstore bundle v0.3 JSON format
    // (camelCase field names, same structure), so pass them through directly.
    let tlog_entries: Vec<serde_json::Value> = verification_material
        .get("transparency_entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Construct a sigstore v0.3 bundle JSON
    let bundle_json = serde_json::json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {
            "certificate": { "rawBytes": certificate },
            "tlogEntries": tlog_entries,
            "timestampVerificationData": {}
        },
        "dsseEnvelope": {
            "payload": statement,
            "payloadType": "application/vnd.in-toto+json",
            "signatures": [{ "sig": signature }]
        }
    });

    let bundle_str = serde_json::to_string(&bundle_json)
        .map_err(|e| err(&format!("Failed to serialize bundle: {}", e)))?;

    sigstore_types::Bundle::from_json(&bundle_str)
        .map_err(|e| err(&format!("Failed to parse converted bundle: {}", e)))
}

/// Download an attestation bundle from a URL.
///
/// Returns the raw response body — can be a standard sigstore bundle
/// or a PyPI PEP 740 provenance response.
async fn download_attestation_bundle(
    client: &BaseClient,
    url: &url::Url,
) -> Result<String, CacheError> {
    let mut request = client.for_host(url).client().get(url.clone());
    if url.host_str() == Some("api.github.com") {
        request = request
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, "rattler-build")
            .header("X-GitHub-Api-Version", "2022-11-28");
    }

    let response = request
        .send()
        .await
        .map_err(|e| CacheError::AttestationBundleDownload {
            url: url.to_string(),
            reason: e.to_string(),
        })?;

    if !response.status().is_success() {
        return Err(CacheError::AttestationBundleDownload {
            url: url.to_string(),
            reason: format!("HTTP error: {}", response.status()),
        });
    }

    response
        .text()
        .await
        .map_err(|e| CacheError::AttestationBundleDownload {
            url: url.to_string(),
            reason: format!("Failed to read response body: {}", e),
        })
}

/// Verify that an artifact's SHA-256 digest matches at least one subject in
/// the in-toto statement of at least one bundle.
///
/// This is a critical security check: without it, an attestation for a
/// *different* artifact (e.g. a different release) would be accepted as valid.
fn verify_artifact_subject(
    artifact_sha256_hex: &str,
    bundles: &[sigstore_types::Bundle],
) -> Result<(), CacheError> {
    let mut found_any_subjects = false;
    let mut found_subject_digests: Vec<String> = Vec::new();

    for bundle in bundles {
        // Extract the DSSE envelope from the bundle
        let envelope = match &bundle.content {
            SignatureContent::DsseEnvelope(envelope) => envelope,
            _ => continue,
        };

        // Only check in-toto statements
        if envelope.payload_type != "application/vnd.in-toto+json" {
            continue;
        }

        // Decode and parse the in-toto statement
        let payload_bytes = envelope.decode_payload();

        let statement: sigstore_types::Statement = match serde_json::from_slice(&payload_bytes) {
            Ok(s) => s,
            Err(_) => continue,
        };

        if statement.subject.is_empty() {
            continue;
        }

        found_any_subjects = true;

        // Check if the artifact hash matches any subject's sha256 digest
        if statement.matches_sha256(artifact_sha256_hex) {
            return Ok(());
        }

        // Collect subject digests for error reporting
        for subject in &statement.subject {
            if let Some(sha256) = subject.digest.sha256.as_ref() {
                found_subject_digests.push(format!("{}  (name: {})", sha256, subject.name));
            }
        }
    }

    if !found_any_subjects {
        return Err(CacheError::AttestationVerification(
            "attestation bundle does not contain any in-toto subjects to verify against"
                .to_string(),
        ));
    }

    let mut msg = format!(
        "artifact SHA-256 ({}) does not match any subject in the attestation",
        artifact_sha256_hex,
    );
    if !found_subject_digests.is_empty() {
        msg.push_str("\n  subjects in attestation:");
        for digest in &found_subject_digests {
            msg.push_str(&format!("\n    - {}", digest));
        }
    }
    Err(CacheError::AttestationVerification(msg))
}

/// Verify an attestation for a downloaded artifact.
///
/// Downloads the attestation bundle (either from an explicit URL or discovered
/// from PyPI/GitHub), loads the appropriate Sigstore trusted root, and verifies
/// the signature and each identity check.
///
/// Always verifies that the artifact's SHA-256 digest matches a subject in the
/// attestation's in-toto statement — regardless of whether publisher identity
/// checks are configured.
///
/// Identity matching uses repository-boundary prefix semantics: the expected
/// identity (e.g. `https://github.com/pallets/flask`) must either equal the
/// actual certificate identity or be followed by `/` or `@` in the actual
/// identity (e.g. `https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1`).
fn identity_matches(expected: &str, actual: &str) -> bool {
    if actual == expected {
        return true;
    }

    let Some(suffix) = actual.strip_prefix(expected) else {
        return false;
    };

    // Require a repository boundary so `github:pallets/flask` does not match
    // identities for repositories such as `pallets/flask-cors`.
    suffix.starts_with('/') || suffix.starts_with('@')
}

/// Return repository package-URL subjects from GitHub release statements.
///
/// GitHub's release attester has the fixed certificate identity
/// `https://dotcom.releases.github.com`; the repository identity is instead
/// cryptographically bound into the signed subject as a package URL.
fn github_release_repository_subjects(bundles: &[sigstore_types::Bundle]) -> Vec<String> {
    let mut repositories = Vec::new();
    for bundle in bundles {
        let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
            continue;
        };
        if envelope.payload_type != "application/vnd.in-toto+json" {
            continue;
        }
        let Ok(statement) = serde_json::from_slice::<serde_json::Value>(&envelope.decode_payload())
        else {
            continue;
        };
        let Some(subjects) = statement.get("subject").and_then(|value| value.as_array()) else {
            continue;
        };
        repositories.extend(subjects.iter().filter_map(|subject| {
            subject
                .get("uri")
                .and_then(|value| value.as_str())
                .filter(|uri| uri.starts_with("pkg:github/"))
                .map(ToOwned::to_owned)
        }));
    }
    repositories.sort();
    repositories.dedup();
    repositories
}

fn github_release_matches_repository(
    repository_subjects: &[String],
    expected_identity: &str,
) -> bool {
    let Some(repository) = expected_identity.strip_prefix("https://github.com/") else {
        return false;
    };
    let expected_uri_prefix = format!("pkg:github/{repository}@");
    repository_subjects
        .iter()
        .any(|uri| uri.starts_with(&expected_uri_prefix))
}

pub(crate) async fn verify_attestation(
    client: &BaseClient,
    file_path: &Path,
    source_url: &url::Url,
    attestation_config: &AttestationVerification,
) -> Result<(), CacheError> {
    // Read and hash the artifact before deriving the URL: GitHub's API can
    // filter attestations by the exact release asset digest.
    let artifact_bytes = fs_err::tokio::read(file_path).await?;
    let artifact_sha256_hex = hex::encode(Sha256::digest(&artifact_bytes));

    // Determine bundle URL: explicit, or auto-derive from PyPI/GitHub.
    let bundle_url = if let Some(url) = &attestation_config.bundle_url {
        Some(url.clone())
    } else {
        derive_pypi_provenance_url(source_url)
            .or_else(|| derive_github_attestations_url(source_url, &artifact_sha256_hex))
    };

    let bundle_url = bundle_url.ok_or_else(|| {
        CacheError::InvalidAttestationBundle(
            "No bundle_url provided and could not auto-derive one (supported sources are PyPI files and GitHub release assets)".to_string(),
        )
    })?;

    tracing::info!("Downloading attestation bundle from {}", bundle_url);
    let response_json = download_attestation_bundle(client, &bundle_url).await?;

    // Parse before selecting a trust domain: GitHub immutable-release
    // attestations use GitHub's own Sigstore instance rather than the public one.
    let parsed = parse_attestation_response(&response_json)?;
    let trusted_root = if parsed.from_github {
        TrustedRoot::from_tuf(sigstore_trust_root::TufConfig::github()).await
    } else {
        TrustedRoot::production().await
    }
    .map_err(|e| {
        CacheError::SigstoreTrustRoot(format!("Failed to load Sigstore trusted root via TUF: {e}"))
    })?;

    // Always verify the artifact's digest matches a subject in the attestation.
    // This prevents accepting an attestation for a different file (e.g., from a
    // different release or a different artifact entirely).
    verify_artifact_subject(&artifact_sha256_hex, &parsed.bundles)?;

    // A bundle URL without publisher constraints must still perform
    // cryptographic verification rather than only inspecting the signed payload.
    if attestation_config.identity_checks.is_empty() {
        let mut errors = Vec::new();
        let mut verified = false;
        for bundle in &parsed.bundles {
            let policy = if parsed.from_github {
                VerificationPolicy::default()
                    .require_identity("https://dotcom.releases.github.com")
                    .skip_tlog()
                    .skip_sct()
            } else if parsed.from_pypi {
                VerificationPolicy::default().skip_tlog()
            } else {
                VerificationPolicy::default()
            };
            match verify(artifact_bytes.as_slice(), bundle, &policy, &trusted_root) {
                Ok(_) => {
                    verified = true;
                    break;
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        if !verified {
            return Err(CacheError::AttestationVerification(format!(
                "attestation signature verification failed: {}",
                errors.join("; ")
            )));
        }
    }

    // For each required identity check, find a matching bundle and verify it
    for check in &attestation_config.identity_checks {
        let mut matched = false;
        let mut found_identities: Vec<String> = Vec::new();
        let mut verification_errors: Vec<String> = Vec::new();
        let repository_subjects = if parsed.from_github {
            github_release_repository_subjects(&parsed.bundles)
        } else {
            Vec::new()
        };
        let repository_matches = !parsed.from_github
            || github_release_matches_repository(&repository_subjects, &check.identity);

        for bundle in &parsed.bundles {
            // GitHub immutable releases are signed by GitHub's release service,
            // not by the repository's workflow identity. Repository ownership is
            // checked in the signed package-URL subject above. GitHub bundles use
            // an RFC 3161 timestamp and do not contain Rekor or public CT entries.
            let policy = if parsed.from_github {
                VerificationPolicy::default()
                    .require_identity("https://dotcom.releases.github.com")
                    .skip_tlog()
                    .skip_sct()
            } else {
                // Verify with just the issuer in the policy — we do prefix
                // matching on identity ourselves.
                let mut policy = VerificationPolicy::default().require_issuer(check.issuer.clone());
                if parsed.from_pypi {
                    // We cannot reconstruct the canonicalized Rekor body from
                    // PEP 740's representation.
                    policy = policy.skip_tlog();
                }
                policy
            };

            match verify(artifact_bytes.as_slice(), bundle, &policy, &trusted_root) {
                Ok(result) => {
                    if let Some(ref actual_identity) = result.identity {
                        let identity_ok = if parsed.from_github {
                            // The policy checked the release-service certificate;
                            // also require the repository in the signed statement.
                            repository_matches
                        } else {
                            identity_matches(&check.identity, actual_identity)
                        };
                        if identity_ok {
                            tracing::info!(
                                "\u{2714} Attestation verified (identity={})",
                                actual_identity,
                            );
                            matched = true;
                            break;
                        } else {
                            found_identities.push(actual_identity.clone());
                        }
                    }
                }
                Err(e) => {
                    verification_errors.push(e.to_string());
                }
            }
        }

        if !matched {
            let publisher = check
                .identity
                .trim_start_matches("https://github.com/")
                .trim_start_matches("https://gitlab.com/");
            let mut msg = if parsed.from_github {
                let mut msg = format!(
                    "attestation repository identity mismatch for publisher '{publisher}'\n  expected repository identity: {}\n  signing certificate identity: https://dotcom.releases.github.com",
                    check.identity,
                );
                if repository_subjects.is_empty() {
                    msg.push_str("\n  found signed repository subjects: none");
                } else {
                    msg.push_str("\n  found signed repository subjects:");
                    for subject in &repository_subjects {
                        msg.push_str(&format!("\n    - {subject}"));
                    }
                }
                msg
            } else {
                format!(
                    "attestation identity mismatch for publisher '{publisher}'\n  expected identity prefix: {}\n  expected issuer: {}",
                    check.identity, check.issuer,
                )
            };
            if !found_identities.is_empty() && !parsed.from_github {
                msg.push_str("\n  found identities in attestation:");
                for id in &found_identities {
                    msg.push_str(&format!("\n    - {}", id));
                }
            }
            if !verification_errors.is_empty() {
                for err in &verification_errors {
                    msg.push_str(&format!("\n  verification error: {}", err));
                }
            }
            return Err(CacheError::AttestationVerification(msg));
        }
    }

    tracing::info!(
        "\u{2714} All attestation checks passed for {}",
        file_path
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_else(|| file_path.to_string_lossy())
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_pypi_provenance_url_pypi_io() {
        let url =
            url::Url::parse("https://pypi.io/packages/source/f/flask/flask-3.1.1.tar.gz").unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask/3.1.1/flask-3.1.1.tar.gz/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_pythonhosted() {
        let url = url::Url::parse(
            "https://files.pythonhosted.org/packages/source/f/flask/flask-3.1.1.tar.gz",
        )
        .unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask/3.1.1/flask-3.1.1.tar.gz/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_normalizes_name() {
        let url =
            url::Url::parse("https://pypi.io/packages/source/F/Flask-CORS/Flask-CORS-4.0.0.tar.gz")
                .unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask-cors/4.0.0/Flask-CORS-4.0.0.tar.gz/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_non_pypi() {
        let url =
            url::Url::parse("https://github.com/pallets/flask/archive/v3.1.1.tar.gz").unwrap();
        assert!(derive_pypi_provenance_url(&url).is_none());
    }

    #[test]
    fn test_derive_pypi_provenance_url_zip() {
        let url =
            url::Url::parse("https://pypi.io/packages/source/f/flask/flask-3.1.1.zip").unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask/3.1.1/flask-3.1.1.zip/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_wheel() {
        let url = url::Url::parse(
            "https://files.pythonhosted.org/packages/aa/bb/charset_normalizer-3.4.0-py3-none-any.whl",
        )
        .unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/charset-normalizer/3.4.0/charset_normalizer-3.4.0-py3-none-any.whl/provenance"
        );
    }

    #[test]
    fn test_derive_github_attestations_url() {
        let url = url::Url::parse(
            "https://github.com/prefix-dev/rattler-build/releases/download/v1.0.0/source.tar.gz",
        )
        .unwrap();
        let result = derive_github_attestations_url(&url, "abc123").unwrap();
        assert_eq!(result.host_str(), Some("api.github.com"));
        assert_eq!(
            result.path(),
            "/repos/prefix-dev/rattler-build/attestations/sha256:abc123"
        );
        let query: std::collections::HashMap<_, _> = result.query_pairs().collect();
        assert_eq!(query.get("predicate_type").unwrap(), "release");
    }

    #[test]
    fn test_github_generated_archive_is_not_auto_derived() {
        let url = url::Url::parse(
            "https://github.com/prefix-dev/rattler-build/archive/refs/tags/v1.0.0.tar.gz",
        )
        .unwrap();
        assert!(derive_github_attestations_url(&url, "abc123").is_none());
    }

    #[test]
    fn test_parse_attestation_response_sigstore_bundle() {
        // A sigstore bundle has a "mediaType" field
        let json = r#"{
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {
                "certificate": { "rawBytes": "dGVzdA==" },
                "tlogEntries": [],
                "timestampVerificationData": {}
            },
            "dsseEnvelope": {
                "payload": "dGVzdA==",
                "payloadType": "application/vnd.in-toto+json",
                "signatures": [{ "sig": "dGVzdA==" }]
            }
        }"#;
        let parsed = parse_attestation_response(json).unwrap();
        assert_eq!(parsed.bundles.len(), 1);
        assert!(!parsed.from_pypi);
        assert!(!parsed.from_github);
    }

    #[test]
    fn test_parse_attestation_response_pypi_provenance() {
        // A PyPI provenance response has "attestation_bundles"
        let json = r#"{
            "version": 1,
            "attestation_bundles": [
                {
                    "publisher": { "kind": "GitHub", "repository": "pallets/flask" },
                    "attestations": [
                        {
                            "version": 1,
                            "envelope": {
                                "statement": "dGVzdA==",
                                "signature": "dGVzdA=="
                            },
                            "verification_material": {
                                "certificate": "dGVzdA==",
                                "transparency_entries": []
                            }
                        }
                    ]
                }
            ]
        }"#;
        let parsed = parse_attestation_response(json).unwrap();
        assert_eq!(parsed.bundles.len(), 1);
        assert!(parsed.from_pypi);
        assert!(!parsed.from_github);
    }

    #[test]
    fn test_parse_attestation_response_github() {
        let bundle = make_bundle_with_subjects(&[("source.tar.gz", "abc123")]);
        let bundle_json: serde_json::Value =
            serde_json::from_str(&bundle.to_json().unwrap()).unwrap();
        let response = serde_json::json!({
            "attestations": [{ "id": 42, "bundle": bundle_json }]
        });
        let parsed = parse_attestation_response(&response.to_string()).unwrap();
        assert_eq!(parsed.bundles.len(), 1);
        assert!(!parsed.from_pypi);
        assert!(parsed.from_github);
    }

    #[test]
    fn test_parse_empty_github_response_explains_generated_archives() {
        let err = parse_attestation_response(r#"{ "attestations": [] }"#).unwrap_err();
        assert!(
            err.to_string()
                .contains("dynamically generated source archives")
        );
    }

    #[test]
    fn test_parse_attestation_response_unrecognized_format() {
        let json = r#"{ "foo": "bar" }"#;
        assert!(parse_attestation_response(json).is_err());
    }

    #[test]
    fn test_identity_matches_github_workflow_identity() {
        assert!(identity_matches(
            "https://github.com/pallets/flask",
            "https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1"
        ));
    }

    #[test]
    fn test_identity_matches_exact_identity() {
        assert!(identity_matches(
            "https://github.com/pallets/flask",
            "https://github.com/pallets/flask"
        ));
    }

    #[test]
    fn test_identity_rejects_repository_prefix_collision() {
        assert!(!identity_matches(
            "https://github.com/pallets/flask",
            "https://github.com/pallets/flask-cors/.github/workflows/release.yml@refs/tags/4.0.0"
        ));
    }

    /// Helper to create a minimal sigstore bundle with an in-toto statement.
    fn make_bundle_with_raw_subjects(
        subject_json: Vec<serde_json::Value>,
    ) -> sigstore_types::Bundle {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let statement = serde_json::json!({
            "_type": "https://in-toto.io/Statement/v1",
            "subject": subject_json,
            "predicateType": "https://in-toto.io/attestation/release/v0.2",
            "predicate": {}
        });

        let payload = STANDARD.encode(serde_json::to_string(&statement).unwrap());

        let bundle_json = serde_json::json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {
                "certificate": { "rawBytes": "dGVzdA==" },
                "tlogEntries": [],
                "timestampVerificationData": {}
            },
            "dsseEnvelope": {
                "payload": payload,
                "payloadType": "application/vnd.in-toto+json",
                "signatures": [{ "sig": "dGVzdA==" }]
            }
        });

        sigstore_types::Bundle::from_json(&serde_json::to_string(&bundle_json).unwrap()).unwrap()
    }

    /// Create a bundle whose subjects are artifact names and SHA-256 pairs.
    fn make_bundle_with_subjects(subjects: &[(&str, &str)]) -> sigstore_types::Bundle {
        make_bundle_with_raw_subjects(
            subjects
                .iter()
                .map(|(name, sha256)| {
                    serde_json::json!({
                        "name": name,
                        "digest": { "sha256": sha256 }
                    })
                })
                .collect(),
        )
    }

    #[test]
    fn test_github_release_repository_subject_matches() {
        let bundle = make_bundle_with_raw_subjects(vec![serde_json::json!({
            "uri": "pkg:github/astral-sh/uv@0.12.1",
            "digest": { "sha1": "abc123" }
        })]);
        let subjects = github_release_repository_subjects(&[bundle]);
        assert_eq!(subjects, ["pkg:github/astral-sh/uv@0.12.1"]);
        assert!(github_release_matches_repository(
            &subjects,
            "https://github.com/astral-sh/uv"
        ));
        assert!(!github_release_matches_repository(
            &subjects,
            "https://github.com/astral-sh/uv-extra"
        ));
    }

    #[test]
    fn test_verify_artifact_subject_matching_digest() {
        let artifact = b"hello world";
        let sha256_hex = hex::encode(Sha256::digest(artifact));
        let bundle = make_bundle_with_subjects(&[("test.tar.gz", &sha256_hex)]);
        assert!(verify_artifact_subject(&sha256_hex, &[bundle]).is_ok());
    }

    #[test]
    fn test_verify_artifact_subject_mismatched_digest() {
        let sha256_hex = hex::encode(Sha256::digest(b"hello world"));
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let bundle = make_bundle_with_subjects(&[("test.tar.gz", wrong_hash)]);
        let err = verify_artifact_subject(&sha256_hex, &[bundle]).unwrap_err();
        assert!(
            err.to_string().contains("does not match any subject"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_verify_artifact_subject_multiple_subjects_one_matches() {
        let sha256_hex = hex::encode(Sha256::digest(b"my artifact"));
        let wrong_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let bundle = make_bundle_with_subjects(&[
            ("other.tar.gz", wrong_hash),
            ("correct.tar.gz", &sha256_hex),
        ]);
        assert!(verify_artifact_subject(&sha256_hex, &[bundle]).is_ok());
    }

    #[test]
    fn test_verify_artifact_subject_empty_subjects() {
        let sha256_hex = hex::encode(Sha256::digest(b"test"));
        let bundle = make_bundle_with_subjects(&[]);
        let err = verify_artifact_subject(&sha256_hex, &[bundle]).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not contain any in-toto subjects"),
            "unexpected error: {}",
            err
        );
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn test_uv_github_release_attestation() {
        let source_url = url::Url::parse(
            "https://github.com/astral-sh/uv/releases/download/0.12.1/source.tar.gz",
        )
        .unwrap();
        let client = BaseClient::builder().timeout(300).build();
        let bytes = client
            .for_host(&source_url)
            .client()
            .get(source_url.clone())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs_err::tokio::write(temp.path(), bytes).await.unwrap();
        let config = AttestationVerification {
            bundle_url: None,
            identity_checks: vec![crate::source::IdentityCheck {
                identity: "https://github.com/astral-sh/uv".to_string(),
                issuer: "https://token.actions.githubusercontent.com".to_string(),
            }],
        };
        verify_attestation(&client, temp.path(), &source_url, &config)
            .await
            .unwrap();

        let wrong_config = AttestationVerification {
            bundle_url: None,
            identity_checks: vec![crate::source::IdentityCheck {
                identity: "https://github.com/astral-sh/xv".to_string(),
                issuer: "https://token.actions.githubusercontent.com".to_string(),
            }],
        };
        let error = verify_attestation(&client, temp.path(), &source_url, &wrong_config)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected repository identity: https://github.com/astral-sh/xv"));
        assert!(error.contains("pkg:github/astral-sh/uv@0.12.1"));
        assert!(error.contains("signing certificate identity: https://dotcom.releases.github.com"));
    }
}
