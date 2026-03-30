//! Sigstore attestation verification
//!
//! This module contains all sigstore-related functionality for verifying
//! attestation bundles against source artifacts. It handles both standard
//! sigstore bundles and PyPI PEP 740 provenance responses.

use std::path::Path;

use sigstore_trust_root::TrustedRoot;
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

    // Extract project name and version from filename
    // Filenames are typically: {project}-{version}.tar.gz or {project}-{version}.whl etc.
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".tar.bz2"))
        .or_else(|| filename.strip_suffix(".zip"))
        .or_else(|| filename.strip_suffix(".whl"))?;

    // Split on the last '-' to separate project from version
    let (project, version) = stem.rsplit_once('-')?;

    // Normalize project name (PEP 503: replace [-_.] with -)
    let normalized_project = project.to_lowercase().replace(['-', '_', '.'], "-");

    let provenance_url = format!(
        "https://pypi.org/integrity/{}/{}/{}/provenance",
        normalized_project, version, filename
    );
    url::Url::parse(&provenance_url).ok()
}

/// Result of parsing an attestation response.
struct ParsedAttestations {
    bundles: Vec<sigstore_types::Bundle>,
    /// Whether these bundles were converted from PyPI PEP 740 provenance format.
    /// PyPI-converted bundles lack canonicalized rekor bodies so transparency log
    /// verification must be skipped.
    from_pypi: bool,
}

/// Parse an attestation response into one or more sigstore bundles.
///
/// Handles two formats:
/// 1. **Standard sigstore bundle** (`.sigstore.json`): has a `mediaType` field,
///    parsed directly via `Bundle::from_json`.
/// 2. **PyPI PEP 740 provenance response**: has an `attestation_bundles` array,
///    each containing `attestations` that are converted to sigstore bundles.
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
        });
    }

    Err(CacheError::InvalidAttestationBundle(
        "Unrecognized attestation format: expected sigstore bundle or PyPI provenance response"
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
    let response = client
        .for_host(url)
        .get(url.clone())
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

/// Verify an attestation for a downloaded artifact.
///
/// Downloads the attestation bundle (either from an explicit URL or auto-derived
/// from PyPI), loads the Sigstore trusted root, and verifies each identity check.
///
/// Identity matching uses **prefix** semantics: the expected identity (e.g.
/// `https://github.com/pallets/flask`) must be a prefix of the actual certificate
/// identity (e.g. `https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1`).
pub(crate) async fn verify_attestation(
    client: &BaseClient,
    file_path: &Path,
    source_url: &url::Url,
    attestation_config: &AttestationVerification,
) -> Result<(), CacheError> {
    // Determine bundle URL: explicit, or auto-derive from PyPI source URL
    let bundle_url = if let Some(url) = &attestation_config.bundle_url {
        Some(url.clone())
    } else {
        derive_pypi_provenance_url(source_url)
    };

    let bundle_url = bundle_url.ok_or_else(|| {
        CacheError::InvalidAttestationBundle(
            "No bundle_url provided and could not auto-derive one (not a PyPI source)".to_string(),
        )
    })?;

    tracing::info!("Downloading attestation bundle from {}", bundle_url);
    let response_json = download_attestation_bundle(client, &bundle_url).await?;

    // Load the production Sigstore trusted root (embedded, no network needed)
    let trusted_root = TrustedRoot::production().map_err(|e| {
        CacheError::SigstoreTrustRoot(format!("Failed to load Sigstore trusted root: {}", e))
    })?;

    // Read the artifact for verification
    let artifact_bytes = fs_err::tokio::read(file_path).await?;

    // Parse the response: could be a plain sigstore bundle or a PyPI provenance response
    let parsed = parse_attestation_response(&response_json)?;

    // For each required identity check, find a matching bundle and verify it
    for check in &attestation_config.identity_checks {
        let mut matched = false;
        let mut found_identities: Vec<String> = Vec::new();
        let mut verification_errors: Vec<String> = Vec::new();

        for bundle in &parsed.bundles {
            // Verify with just the issuer in the policy — we do prefix matching on identity ourselves.
            // For PyPI-converted bundles, skip tlog verification since we can't reconstruct
            // the canonicalized rekor body from the PEP 740 format.
            let mut policy = VerificationPolicy::default().require_issuer(check.issuer.clone());
            if parsed.from_pypi {
                policy = policy.skip_tlog();
            }

            match verify(artifact_bytes.as_slice(), bundle, &policy, &trusted_root) {
                Ok(result) => {
                    if let Some(ref actual_identity) = result.identity {
                        // Prefix match: expected identity must be a prefix of the actual identity
                        if actual_identity.starts_with(&check.identity) {
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
            let mut msg = format!(
                "attestation identity mismatch for publisher '{}'\n  expected identity prefix: {}\n  expected issuer: {}",
                check
                    .identity
                    .trim_start_matches("https://github.com/")
                    .trim_start_matches("https://gitlab.com/"),
                check.identity,
                check.issuer,
            );
            if !found_identities.is_empty() {
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
    }

    #[test]
    fn test_parse_attestation_response_unrecognized_format() {
        let json = r#"{ "foo": "bar" }"#;
        assert!(parse_attestation_response(json).is_err());
    }
}
