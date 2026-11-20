//! Sigstore attestation verification
//!
//! This module verifies attestation bundles against downloaded source
//! artifacts. It understands three response formats:
//!
//! - standard Sigstore bundles (`.sigstore.json`),
//! - PyPI PEP 740 provenance responses (`/integrity/.../provenance`), and
//! - GitHub's repository attestations API (used for immutable release
//!   attestations of uploaded release assets).
//!
//! Every accepted attestation must:
//!
//! 1. cryptographically verify against the trust root of its Sigstore
//!    instance (certificate chain at a verified signing time, transparency log
//!    inclusion or RFC 3161 timestamp, and the DSSE signature),
//! 2. bind the downloaded artifact's SHA-256 digest as a subject, and
//! 3. if publishers are configured, be signed by (or, for GitHub releases,
//!    name) every configured publisher.

use std::path::Path;

use sha2::{Digest, Sha256};
use sigstore_trust_root::{TrustedRoot, TufConfig};
use sigstore_types::{Artifact, Sha256Hash, SignatureContent};
use sigstore_verify::{VerificationPolicy, VerificationResult, verify};

use crate::error::CacheError;
use crate::source::{AttestationVerification, AttestedIds, IdentityCheck};
use rattler_build_networking::BaseClient;

/// The fixed certificate identity of GitHub's immutable release attester.
const GITHUB_RELEASE_IDENTITY: &str = "https://dotcom.releases.github.com";

/// Predicate type prefix for in-toto release attestations.
const RELEASE_PREDICATE_PREFIX: &str = "https://in-toto.io/attestation/release/";

/// Where an attestation came from. This determines the Sigstore trust domain
/// and how publisher identities are matched, and is derived from the URL we
/// requested — never from the shape of the (untrusted) response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttestationKind {
    /// A bundle (or PyPI provenance) signed on the Sigstore public-good
    /// instance, typically by a CI workflow identity.
    PublicGood,
    /// A GitHub immutable release attestation fetched from the repository
    /// attestations API. Signed on GitHub's Sigstore instance by
    /// [`GITHUB_RELEASE_IDENTITY`].
    GitHubRelease,
}

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
    artifact_sha256: &Sha256Hash,
) -> Option<url::Url> {
    if source_url.scheme() != "https" || source_url.host_str()? != "github.com" {
        return None;
    }

    // https://github.com/{owner}/{repo}/releases/download/{tag}/{asset}
    let segments: Vec<_> = source_url.path_segments()?.collect();
    if segments.len() < 6 || segments[2] != "releases" || segments[3] != "download" {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1].strip_suffix(".git").unwrap_or(segments[1]);
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    // The predicate filter is only a server-side convenience; the predicate
    // type is re-checked on the signed statement during verification.
    let mut api_url = url::Url::parse(&format!(
        "https://api.github.com/repos/{owner}/{repo}/attestations/sha256:{}",
        artifact_sha256.to_hex()
    ))
    .ok()?;
    api_url
        .query_pairs_mut()
        .append_pair("predicate_type", "release");
    Some(api_url)
}

/// Whether a URL queries GitHub's repository attestations API for immutable
/// release attestations (`predicate_type=release`).
///
/// Other queries of the same API (e.g. SLSA build provenance created by
/// `actions/attest-build-provenance` in public repositories) are signed on the
/// Sigstore public-good instance by the workflow identity and are verified
/// like any other bundle.
fn is_github_release_attestations_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("api.github.com")
        && url.path_segments().is_some_and(|segments| {
            let segments: Vec<_> = segments.collect();
            segments.len() == 5 && segments[0] == "repos" && segments[3] == "attestations"
        })
        && url
            .query_pairs()
            .any(|(key, value)| key == "predicate_type" && value == "release")
}

/// Parse an attestation response into one or more sigstore bundles.
///
/// Handles three formats:
/// 1. **Standard sigstore bundle** (`.sigstore.json`): has a `mediaType` field,
///    parsed directly via `Bundle::from_json`.
/// 2. **PyPI PEP 740 provenance response**: has an `attestation_bundles` array,
///    each containing `attestations` that are converted to sigstore bundles.
/// 3. **GitHub attestations API response**: has an `attestations` array whose
///    entries each contain a standard bundle in their `bundle` field.
fn parse_attestation_response(json_str: &str) -> Result<Vec<sigstore_types::Bundle>, CacheError> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| CacheError::InvalidAttestationBundle(format!("Invalid JSON: {}", e)))?;

    // If it has a "mediaType" field, it's a standard sigstore bundle
    if value.get("mediaType").is_some() {
        let bundle = sigstore_types::Bundle::from_json(json_str).map_err(|e| {
            CacheError::InvalidAttestationBundle(format!("Failed to parse sigstore bundle: {}", e))
        })?;
        return Ok(vec![bundle]);
    }

    // GitHub's repository attestations API wraps each standard bundle.
    if let Some(attestations) = value.get("attestations").and_then(|v| v.as_array()) {
        let mut bundles = Vec::new();
        for attestation in attestations {
            let Some(bundle_value) = attestation.get("bundle").filter(|v| !v.is_null()) else {
                // Large bundles may only be offered through `bundle_url`
                // (snappy-compressed blob storage). Skip them rather than
                // trusting anything about them.
                tracing::debug!("skipping GitHub attestation without an inline bundle");
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
            return Err(no_github_release_attestation_error());
        }
        return Ok(bundles);
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
        return Ok(bundles);
    }

    Err(CacheError::InvalidAttestationBundle(
        "Unrecognized attestation format: expected a Sigstore bundle, PyPI provenance response, or GitHub attestations API response"
            .to_string(),
    ))
}

fn no_github_release_attestation_error() -> CacheError {
    CacheError::InvalidAttestationBundle(
        "GitHub has no release attestation for this artifact's SHA-256. Only assets uploaded to an \
         immutable release are attested; GitHub's dynamically generated \"Source code\" archives \
         are not, so upload the source archive as a release asset instead"
            .to_string(),
    )
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

    // PyPI transparency_entries already use the sigstore bundle v0.3 JSON
    // format (camelCase field names, including the canonicalized body,
    // inclusion proof and checkpoint), so pass them through directly. This
    // lets the converted bundle undergo full transparency log verification.
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

/// Read a GitHub token from the environment, if any, to raise the GitHub API
/// rate limit (60 requests/hour unauthenticated).
fn github_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|token| !token.trim().is_empty())
}

/// Download an attestation bundle from a URL.
///
/// Returns the raw response body — can be a standard sigstore bundle, a PyPI
/// PEP 740 provenance response, or a GitHub attestations API response.
async fn download_attestation_bundle(
    client: &BaseClient,
    url: &url::Url,
    kind: AttestationKind,
) -> Result<String, CacheError> {
    let download_err = |reason: String| CacheError::AttestationBundleDownload {
        url: url.to_string(),
        reason,
    };

    let is_github_api = url.scheme() == "https" && url.host_str() == Some("api.github.com");
    let send = |token: Option<&str>| {
        let mut request = client.for_host(url).client().get(url.clone());
        if is_github_api {
            request = request
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header(reqwest::header::USER_AGENT, "rattler-build")
                .header("X-GitHub-Api-Version", "2022-11-28");
            // Only ever send the token to the GitHub API host itself.
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
        }
        request.send()
    };

    let token = if is_github_api { github_token() } else { None };
    let mut response = send(token.as_deref())
        .await
        .map_err(|e| download_err(e.to_string()))?;

    // An invalid or expired token turns a public request into a failure;
    // fall back to an unauthenticated request in that case.
    if token.is_some() && response.status() == reqwest::StatusCode::UNAUTHORIZED {
        tracing::warn!(
            "GitHub rejected the token from GITHUB_TOKEN/GH_TOKEN (401); retrying the \
             attestation request unauthenticated"
        );
        response = send(None).await.map_err(|e| download_err(e.to_string()))?;
    }

    let status = response.status();
    if !status.is_success() {
        if kind == AttestationKind::GitHubRelease && status == reqwest::StatusCode::NOT_FOUND {
            return Err(no_github_release_attestation_error());
        }
        if is_github_api
            && (status == reqwest::StatusCode::FORBIDDEN
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
        {
            return Err(download_err(format!(
                "HTTP error: {status}. The GitHub API rate limit may have been exceeded; set \
                 GITHUB_TOKEN (or GH_TOKEN) to authenticate requests"
            )));
        }
        if status == reqwest::StatusCode::NOT_FOUND && url.host_str() == Some("pypi.org") {
            return Err(download_err(format!(
                "HTTP error: {status}. PyPI has no provenance for this file; it was probably \
                 not uploaded with a Trusted Publisher"
            )));
        }
        return Err(download_err(format!("HTTP error: {status}")));
    }

    response
        .text()
        .await
        .map_err(|e| download_err(format!("Failed to read response body: {}", e)))
}

/// Parse the in-toto statement of a bundle, if it carries one.
fn bundle_statement(bundle: &sigstore_types::Bundle) -> Option<serde_json::Value> {
    let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
        return None;
    };
    if envelope.payload_type != "application/vnd.in-toto+json" {
        return None;
    }
    serde_json::from_slice(envelope.payload.as_bytes()).ok()
}

/// Verify that an artifact's SHA-256 digest matches at least one subject in
/// the in-toto statement of at least one bundle.
///
/// This is a pre-flight check that yields a helpful error message. The
/// binding between artifact and *verified* bundle is enforced again by
/// `sigstore_verify::verify` for every bundle that is accepted.
///
/// Message-signature bundles (e.g. produced by `cosign sign-blob`) carry no
/// in-toto statement; their digest binding is checked by `verify` alone.
fn verify_artifact_subject(
    artifact_sha256: &Sha256Hash,
    bundles: &[sigstore_types::Bundle],
) -> Result<(), CacheError> {
    if bundles
        .iter()
        .any(|b| matches!(b.content, SignatureContent::MessageSignature(_)))
    {
        return Ok(());
    }

    let mut found_any_subjects = false;
    let mut found_subject_digests: Vec<String> = Vec::new();

    for bundle in bundles {
        let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
            continue;
        };
        if envelope.payload_type != "application/vnd.in-toto+json" {
            continue;
        }
        let Ok(statement) =
            serde_json::from_slice::<sigstore_types::Statement>(envelope.payload.as_bytes())
        else {
            continue;
        };
        if statement.subject.is_empty() {
            continue;
        }

        found_any_subjects = true;

        if statement.matches_sha256(artifact_sha256) {
            return Ok(());
        }

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
        artifact_sha256,
    );
    if !found_subject_digests.is_empty() {
        msg.push_str("\n  subjects in attestation:");
        for digest in &found_subject_digests {
            msg.push_str(&format!("\n    - {}", digest));
        }
    }
    Err(CacheError::AttestationVerification(msg))
}

/// Case-insensitive ASCII prefix strip (GitHub/GitLab owner and repository
/// names are case-insensitive).
fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Match a certificate identity against an identity check.
///
/// Identity matching uses repository-boundary prefix semantics: the expected
/// identity (e.g. `https://github.com/pallets/flask`) must either equal the
/// actual certificate identity or be followed by `/` or `@` in the actual
/// identity (e.g. `https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1`).
///
/// If a ref constraint is set, the identity must additionally end in
/// `@{ref}`, which is how GitHub Actions and GitLab CI encode the ref of the
/// signing workflow in the certificate SAN.
fn identity_matches(check: &IdentityCheck, actual: &str) -> bool {
    let Some(suffix) = strip_prefix_ignore_ascii_case(actual, &check.identity) else {
        return false;
    };

    // Require a repository boundary so `github:pallets/flask` does not match
    // identities for repositories such as `pallets/flask-cors`.
    if !(suffix.is_empty() || suffix.starts_with('/') || suffix.starts_with('@')) {
        return false;
    }

    // The ref is everything after the *first* `@`: git refs may themselves
    // contain `@`, so splitting at the last one would let a branch named
    // `x@refs/tags/1.0` (SAN `...@refs/heads/x@refs/tags/1.0`) satisfy a
    // `refs/tags/1.0` constraint.
    match &check.ref_constraint {
        None => true,
        Some(git_ref) => suffix
            .split_once('@')
            .is_some_and(|(_, actual_ref)| actual_ref == git_ref),
    }
}

/// Return repository package-URL subjects (`pkg:github/owner/repo@tag`) from
/// a GitHub release statement.
fn github_release_repository_subjects(statement: &serde_json::Value) -> Vec<String> {
    let Some(subjects) = statement.get("subject").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    let mut repositories: Vec<_> = subjects
        .iter()
        .filter_map(|subject| subject.get("uri").and_then(|value| value.as_str()))
        .filter(|uri| uri.starts_with("pkg:github/"))
        .map(ToOwned::to_owned)
        .collect();
    repositories.sort();
    repositories.dedup();
    repositories
}

/// Match the signed repository subjects of a GitHub release statement against
/// an identity check. A ref constraint must be a tag (`refs/tags/{tag}`) and is
/// compared with the release tag in the package URL.
fn github_release_matches(repository_subjects: &[String], check: &IdentityCheck) -> bool {
    let Some(expected_repository) = check.identity.strip_prefix("https://github.com/") else {
        return false;
    };
    let expected_tag = match &check.ref_constraint {
        None => None,
        Some(git_ref) => match git_ref.strip_prefix("refs/tags/") {
            Some(tag) => Some(tag),
            // Release attestations only record the tag.
            None => return false,
        },
    };
    repository_subjects.iter().any(|uri| {
        uri.strip_prefix("pkg:github/")
            .and_then(|package| package.split_once('@'))
            .is_some_and(|(repository, tag)| {
                repository.eq_ignore_ascii_case(expected_repository)
                    && expected_tag.is_none_or(|expected| expected == tag)
            })
    })
}

/// Repository facts beyond name and ref that a publisher can pin: the
/// immutable repository and owner IDs, and the deployment environment.
#[derive(Debug, Default)]
struct Pins {
    repository_id: Option<String>,
    owner_id: Option<String>,
    environment: Option<String>,
}

impl Pins {
    /// Whether every value the check pins is present here and equal.
    fn satisfy(&self, check: &IdentityCheck) -> bool {
        let pinned = |expected: &Option<String>, actual: &Option<String>| {
            expected.is_none() || expected == actual
        };
        pinned(&check.repository_id, &self.repository_id)
            && pinned(&check.owner_id, &self.owner_id)
            && pinned(&check.environment, &self.environment)
    }

    fn describe(&self) -> String {
        [
            ("repository_id", &self.repository_id),
            ("owner_id", &self.owner_id),
            ("environment", &self.environment),
        ]
        .iter()
        .filter_map(|(key, value)| value.as_ref().map(|v| format!(", {key}: {v}")))
        .collect()
    }
}

/// Read a string-or-number field from a JSON object as a string.
fn json_id(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key)? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A bundle that passed cryptographic verification against the artifact.
struct VerifiedBundle<'a> {
    bundle: &'a sigstore_types::Bundle,
    result: VerificationResult,
}

impl VerifiedBundle<'_> {
    fn identity(&self) -> Option<&str> {
        self.result.identity().map(|identity| identity.as_str())
    }

    /// The pinnable repository facts of this bundle. For public-good
    /// attestations they come from the Fulcio CI claims of the verified signing
    /// certificate; for GitHub release attestations from the signed release
    /// predicate, which records no deployment environment.
    fn pins(&self, kind: AttestationKind) -> Pins {
        match kind {
            AttestationKind::GitHubRelease => bundle_statement(self.bundle)
                .and_then(|s| s.get("predicate").cloned())
                .map(|predicate| Pins {
                    repository_id: json_id(&predicate, "repositoryId"),
                    owner_id: json_id(&predicate, "ownerId"),
                    environment: None,
                })
                .unwrap_or_default(),
            AttestationKind::PublicGood => self
                .result
                .certificate()
                .filter(|_| self.result.certificate_verified())
                .map(|cert| Pins {
                    repository_id: cert.ci_claims.source_repository_identifier.clone(),
                    owner_id: cert.ci_claims.source_repository_owner_identifier.clone(),
                    environment: cert.ci_claims.deployment_environment.clone(),
                })
                .unwrap_or_default(),
        }
    }

    fn describe(&self, kind: AttestationKind) -> Vec<String> {
        let pins = self.pins(kind).describe();
        match kind {
            AttestationKind::GitHubRelease => bundle_statement(self.bundle)
                .map(|s| github_release_repository_subjects(&s))
                .unwrap_or_default()
                .into_iter()
                .map(|subject| {
                    if pins.is_empty() {
                        subject
                    } else {
                        format!("{subject} ({})", &pins[2..])
                    }
                })
                .collect(),
            AttestationKind::PublicGood => {
                vec![format!(
                    "{} (issuer: {}{pins})",
                    self.identity().unwrap_or("<none>"),
                    self.result.issuer().unwrap_or("<none>")
                )]
            }
        }
    }

    fn satisfies(&self, check: &IdentityCheck, kind: AttestationKind) -> bool {
        let identity_ok = match kind {
            // The policy already pinned the certificate to GitHub's release
            // attester; the repository is bound by the signed statement of this
            // same bundle. Never combine evidence across bundles.
            AttestationKind::GitHubRelease => bundle_statement(self.bundle).is_some_and(|s| {
                github_release_matches(&github_release_repository_subjects(&s), check)
            }),
            AttestationKind::PublicGood => {
                self.result.issuer() == Some(check.issuer.as_str())
                    && self
                        .identity()
                        .is_some_and(|identity| identity_matches(check, identity))
            }
        };
        identity_ok && self.pins(kind).satisfy(check)
    }
}

/// Cryptographically verify every bundle and return the ones that pass,
/// together with the errors of the ones that did not.
fn verify_bundles<'a>(
    artifact: &Artifact<'_>,
    bundles: &'a [sigstore_types::Bundle],
    kind: AttestationKind,
    trusted_root: &TrustedRoot,
) -> (Vec<VerifiedBundle<'a>>, Vec<String>) {
    let policy = match kind {
        // Full verification: certificate chain, SCT and transparency log
        // inclusion proof. This also applies to PyPI attestations, whose
        // transparency entries carry everything needed.
        AttestationKind::PublicGood => VerificationPolicy::any_identity(),
        // GitHub's Sigstore instance has no public transparency log or CT log.
        // The signing time comes from a verified RFC 3161 timestamp instead.
        AttestationKind::GitHubRelease => VerificationPolicy::any_identity()
            .require_identity(GITHUB_RELEASE_IDENTITY)
            .skip_tlog_unsafe()
            .skip_sct(),
    };

    let mut verified = Vec::new();
    let mut errors = Vec::new();
    for bundle in bundles {
        if kind == AttestationKind::GitHubRelease {
            let predicate_type = bundle_statement(bundle).and_then(|s| {
                s.get("predicateType")
                    .and_then(|p| p.as_str())
                    .map(ToOwned::to_owned)
            });
            if !predicate_type
                .as_deref()
                .is_some_and(|p| p.starts_with(RELEASE_PREDICATE_PREFIX))
            {
                errors.push(format!(
                    "not a release attestation (predicate type: {})",
                    predicate_type.as_deref().unwrap_or("<none>")
                ));
                continue;
            }
        }

        match verify(artifact.clone(), bundle, &policy, trusted_root) {
            Ok(result) => verified.push(VerifiedBundle { bundle, result }),
            Err(e) => errors.push(e.to_string()),
        }
    }
    (verified, errors)
}

/// Verify an attestation for a downloaded artifact.
///
/// Downloads the attestation (from an explicit `bundle_url`, or discovered
/// from PyPI / the GitHub attestations API), verifies it cryptographically
/// against the right Sigstore trust root, and checks every configured
/// publisher identity against a verified bundle.
pub(crate) async fn verify_attestation(
    client: &BaseClient,
    file_path: &Path,
    source_url: &url::Url,
    attestation_config: &AttestationVerification,
) -> Result<Vec<AttestedIds>, CacheError> {
    // Hash the artifact first: GitHub's API is queried by the asset digest.
    let artifact_bytes = fs_err::tokio::read(file_path).await?;
    let artifact_sha256 = Sha256Hash::new(Sha256::digest(&artifact_bytes).into());

    let bundle_url = match &attestation_config.bundle_url {
        Some(url) => url.clone(),
        None => derive_pypi_provenance_url(source_url)
            .or_else(|| derive_github_attestations_url(source_url, &artifact_sha256))
            .ok_or_else(|| {
                CacheError::InvalidAttestationBundle(
                    "No bundle_url provided and could not auto-derive one (supported sources \
                     are PyPI files and GitHub release assets)"
                        .to_string(),
                )
            })?,
    };
    let kind = if is_github_release_attestations_url(&bundle_url) {
        AttestationKind::GitHubRelease
    } else {
        AttestationKind::PublicGood
    };

    tracing::info!("Downloading attestation bundle from {}", bundle_url);
    let response_json = download_attestation_bundle(client, &bundle_url, kind).await?;

    // Load trust material through TUF so it is fresh rather than embedded.
    let tuf_config = match kind {
        AttestationKind::PublicGood => TufConfig::production(),
        AttestationKind::GitHubRelease => TufConfig::github(),
    };
    let trusted_root = TrustedRoot::from_tuf(tuf_config).await.map_err(|e| {
        CacheError::SigstoreTrustRoot(format!("Failed to load Sigstore trusted root via TUF: {e}"))
    })?;

    let ids = check_attestation_response(
        Artifact::from_blob(&artifact_bytes),
        &artifact_sha256,
        &response_json,
        kind,
        &trusted_root,
        attestation_config,
    )?;

    tracing::info!(
        "\u{2714} All attestation checks passed for {}",
        file_path
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_else(|| file_path.to_string_lossy())
    );
    Ok(ids)
}

/// Verify a downloaded attestation response against an artifact: parse it,
/// verify every bundle cryptographically against `trusted_root`, and check
/// every configured publisher against a verified bundle.
///
/// Returns, for each publisher in order, the repository and owner IDs of the
/// bundle that satisfied it.
fn check_attestation_response(
    artifact: Artifact<'_>,
    artifact_sha256: &Sha256Hash,
    response_json: &str,
    kind: AttestationKind,
    trusted_root: &TrustedRoot,
    attestation_config: &AttestationVerification,
) -> Result<Vec<AttestedIds>, CacheError> {
    let bundles = parse_attestation_response(response_json)?;

    // Give a precise error early if the attestation is for a different file.
    verify_artifact_subject(artifact_sha256, &bundles)?;

    let (verified, errors) = verify_bundles(&artifact, &bundles, kind, trusted_root);
    if verified.is_empty() {
        let mut msg = String::from("no attestation bundle passed signature verification");
        for err in &errors {
            msg.push_str(&format!("\n  verification error: {err}"));
        }
        return Err(CacheError::AttestationVerification(msg));
    }

    if attestation_config.identity_checks.is_empty() {
        tracing::warn!(
            "attestation was verified cryptographically, but no `publishers` are configured, \
             so any signer is accepted"
        );
    }

    // Every configured publisher must be satisfied by a verified bundle.
    let mut attested = Vec::with_capacity(attestation_config.identity_checks.len());
    for check in &attestation_config.identity_checks {
        if let Some(bundle) = verified.iter().find(|b| b.satisfies(check, kind)) {
            tracing::info!(
                "\u{2714} Attestation verified (identity={})",
                bundle.identity().unwrap_or("<none>")
            );
            let pins = bundle.pins(kind);
            if check.repository_id.is_none()
                && check.owner_id.is_none()
                && let Some(hint) = pin_hint(check, &pins)
            {
                tracing::info!(
                    "Tip: pin this publisher to its immutable repository and owner IDs, which a \
                     renamed or recreated repository cannot match: {hint}"
                );
            }
            attested.push(AttestedIds {
                repository_id: pins.repository_id,
                owner_id: pins.owner_id,
            });
            continue;
        }

        let publisher = check
            .identity
            .trim_start_matches("https://github.com/")
            .trim_start_matches("https://gitlab.com/");
        let mut found: Vec<String> = verified.iter().flat_map(|b| b.describe(kind)).collect();
        found.sort();
        found.dedup();

        let mut msg = match kind {
            AttestationKind::GitHubRelease => format!(
                "attestation repository identity mismatch for publisher '{publisher}'\n  expected repository identity: {}\n  signing certificate identity: {GITHUB_RELEASE_IDENTITY}",
                check.identity,
            ),
            AttestationKind::PublicGood => format!(
                "attestation identity mismatch for publisher '{publisher}'\n  expected identity prefix: {}\n  expected issuer: {}",
                check.identity, check.issuer,
            ),
        };
        if let Some(git_ref) = &check.ref_constraint {
            msg.push_str(&format!("\n  expected ref: {git_ref}"));
        }
        for (key, value) in [
            ("repository_id", &check.repository_id),
            ("owner_id", &check.owner_id),
            ("environment", &check.environment),
        ] {
            if let Some(value) = value {
                msg.push_str(&format!("\n  expected {key}: {value}"));
            }
        }
        if kind == AttestationKind::GitHubRelease && check.environment.is_some() {
            msg.push_str(
                "\n  note: GitHub release attestations do not record a deployment environment, \
                 so an `environment` pin can never match them",
            );
        }
        let label = match kind {
            AttestationKind::GitHubRelease => "found signed repository subjects",
            AttestationKind::PublicGood => "found identities in attestation",
        };
        if found.is_empty() {
            msg.push_str(&format!("\n  {label}: none"));
        } else {
            msg.push_str(&format!("\n  {label}:"));
            for item in &found {
                msg.push_str(&format!("\n    - {item}"));
            }
        }
        for err in &errors {
            msg.push_str(&format!("\n  verification error: {err}"));
        }
        return Err(CacheError::AttestationVerification(msg));
    }

    Ok(attested)
}

/// How to pin `check` to the IDs in `pins`, in the form the recipe most likely
/// uses: the compact string, or the mapping keys when an `environment` shows
/// the mapping form is in use.
fn pin_hint(check: &IdentityCheck, pins: &Pins) -> Option<String> {
    let (repository_id, owner_id) = (pins.repository_id.as_ref()?, pins.owner_id.as_ref()?);
    if check.environment.is_some() {
        return Some(format!(
            "add `repository_id: {repository_id}` and `owner_id: {owner_id}`"
        ));
    }
    let (provider, path) = match check.identity.strip_prefix("https://github.com/") {
        Some(path) => ("github", path),
        None => (
            "gitlab",
            check.identity.strip_prefix("https://gitlab.com/")?,
        ),
    };
    let (owner, repo) = path.split_once('/')?;
    let mut hint = format!("`{provider}:{owner}({owner_id})/{repo}({repository_id})`");
    if check.ref_constraint.is_some() {
        hint.push_str(" (keep your `@ref`)");
    }
    Some(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(identity: &str, ref_constraint: Option<&str>) -> IdentityCheck {
        IdentityCheck {
            identity: identity.to_string(),
            issuer: "https://token.actions.githubusercontent.com".to_string(),
            ref_constraint: ref_constraint.map(ToOwned::to_owned),
            repository_id: None,
            owner_id: None,
            environment: None,
        }
    }

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

    fn sha(hex: &str) -> Sha256Hash {
        Sha256Hash::from_hex(hex).unwrap()
    }

    const ABC_SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    #[test]
    fn test_derive_github_attestations_url() {
        let url = url::Url::parse(
            "https://github.com/prefix-dev/rattler-build/releases/download/v1.0.0/source.tar.gz",
        )
        .unwrap();
        let result = derive_github_attestations_url(&url, &sha(ABC_SHA)).unwrap();
        assert_eq!(result.host_str(), Some("api.github.com"));
        assert_eq!(
            result.path(),
            format!("/repos/prefix-dev/rattler-build/attestations/sha256:{ABC_SHA}")
        );
        let query: std::collections::HashMap<_, _> = result.query_pairs().collect();
        assert_eq!(query.get("predicate_type").unwrap(), "release");
        assert!(is_github_release_attestations_url(&result));
    }

    #[test]
    fn test_github_generated_archive_is_not_auto_derived() {
        let url = url::Url::parse(
            "https://github.com/prefix-dev/rattler-build/archive/refs/tags/v1.0.0.tar.gz",
        )
        .unwrap();
        assert!(derive_github_attestations_url(&url, &sha(ABC_SHA)).is_none());
    }

    #[test]
    fn test_github_attestations_api_url_detection() {
        let bundle =
            url::Url::parse("https://github.com/o/r/releases/download/v1/x.sigstore.json").unwrap();
        assert!(!is_github_release_attestations_url(&bundle));
        let api = url::Url::parse(
            "https://api.github.com/repos/o/r/attestations/sha256:abc?predicate_type=release",
        )
        .unwrap();
        assert!(is_github_release_attestations_url(&api));
        // Build provenance from the same API is a public-good attestation.
        let provenance = url::Url::parse(
            "https://api.github.com/repos/o/r/attestations/sha256:abc?predicate_type=provenance",
        )
        .unwrap();
        assert!(!is_github_release_attestations_url(&provenance));
        let unfiltered =
            url::Url::parse("https://api.github.com/repos/o/r/attestations/sha256:abc").unwrap();
        assert!(!is_github_release_attestations_url(&unfiltered));
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
        assert_eq!(parsed.len(), 1);
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
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn test_parse_attestation_response_github() {
        let bundle = make_bundle_with_subjects(&[("source.tar.gz", ABC_SHA)]);
        let bundle_json: serde_json::Value =
            serde_json::from_str(&bundle.to_json().unwrap()).unwrap();
        let response = serde_json::json!({
            "attestations": [
                { "id": 42, "bundle": bundle_json },
                { "id": 43, "bundle": null, "bundle_url": "https://example.com/b" }
            ]
        });
        let parsed = parse_attestation_response(&response.to_string()).unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn test_parse_empty_github_response_explains_generated_archives() {
        let err = parse_attestation_response(r#"{ "attestations": [] }"#).unwrap_err();
        assert!(err.to_string().contains("dynamically generated"));
    }

    #[test]
    fn test_parse_attestation_response_unrecognized_format() {
        let json = r#"{ "foo": "bar" }"#;
        assert!(parse_attestation_response(json).is_err());
    }

    #[test]
    fn test_identity_matches_github_workflow_identity() {
        assert!(identity_matches(
            &check("https://github.com/pallets/flask", None),
            "https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1"
        ));
    }

    #[test]
    fn test_identity_matches_exact_identity() {
        assert!(identity_matches(
            &check("https://github.com/pallets/flask", None),
            "https://github.com/pallets/flask"
        ));
    }

    #[test]
    fn test_identity_matches_case_insensitive_repository() {
        assert!(identity_matches(
            &check("https://github.com/Pallets/Flask", None),
            "https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1"
        ));
    }

    #[test]
    fn test_identity_rejects_repository_prefix_collision() {
        assert!(!identity_matches(
            &check("https://github.com/pallets/flask", None),
            "https://github.com/pallets/flask-cors/.github/workflows/release.yml@refs/tags/4.0.0"
        ));
    }

    #[test]
    fn test_identity_ref_constraint() {
        let san = "https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1";
        let repo = "https://github.com/pallets/flask";
        assert!(identity_matches(&check(repo, Some("refs/tags/3.1.1")), san));
        assert!(!identity_matches(
            &check(repo, Some("refs/tags/3.1.0")),
            san
        ));
        assert!(!identity_matches(
            &check(repo, Some("refs/heads/main")),
            san
        ));
        // A ref constraint can never be satisfied by an identity without a ref.
        assert!(!identity_matches(
            &check(repo, Some("refs/tags/3.1.1")),
            "https://github.com/pallets/flask"
        ));
        // Git refs may contain `@`: a branch named so that the SAN ends in the
        // constrained tag must not satisfy the tag constraint.
        let branch_san = "https://github.com/pallets/flask/.github/workflows/release.yml@refs/heads/x@refs/tags/3.1.1";
        assert!(!identity_matches(
            &check(repo, Some("refs/tags/3.1.1")),
            branch_san
        ));
        assert!(identity_matches(
            &check(repo, Some("refs/heads/x@refs/tags/3.1.1")),
            branch_san
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

    fn release_subjects(uri: &str) -> Vec<String> {
        let bundle = make_bundle_with_raw_subjects(vec![serde_json::json!({
            "uri": uri,
            "digest": { "sha1": "abc123" }
        })]);
        github_release_repository_subjects(&bundle_statement(&bundle).unwrap())
    }

    #[test]
    fn test_github_release_repository_subject_matches() {
        let subjects = release_subjects("pkg:github/astral-sh/uv@0.12.1");
        assert_eq!(subjects, ["pkg:github/astral-sh/uv@0.12.1"]);
        assert!(github_release_matches(
            &subjects,
            &check("https://github.com/astral-sh/uv", None)
        ));
        assert!(github_release_matches(
            &subjects,
            &check("https://github.com/ASTRAL-SH/UV", None)
        ));
        assert!(!github_release_matches(
            &subjects,
            &check("https://github.com/astral-sh/uv-extra", None)
        ));
    }

    #[test]
    fn test_github_release_ref_constraint() {
        let subjects = release_subjects("pkg:github/astral-sh/uv@0.12.1");
        let repo = "https://github.com/astral-sh/uv";
        assert!(github_release_matches(
            &subjects,
            &check(repo, Some("refs/tags/0.12.1"))
        ));
        assert!(!github_release_matches(
            &subjects,
            &check(repo, Some("refs/tags/0.12.0"))
        ));
        // Branch refs cannot be checked against a release attestation.
        assert!(!github_release_matches(
            &subjects,
            &check(repo, Some("refs/heads/main"))
        ));
    }

    #[test]
    fn test_github_repository_identity_is_scoped_to_one_bundle() {
        let expected_identity = check("https://github.com/victim/project", None);
        assert!(github_release_matches(
            &release_subjects("pkg:github/victim/project@1.0.0"),
            &expected_identity,
        ));
        assert!(!github_release_matches(
            &release_subjects("pkg:github/attacker/project@1.0.0"),
            &expected_identity,
        ));
    }

    #[test]
    fn test_verify_artifact_subject_matching_digest() {
        let artifact = b"hello world";
        let sha256 = Sha256Hash::new(Sha256::digest(artifact).into());
        let bundle = make_bundle_with_subjects(&[("test.tar.gz", &sha256.to_hex())]);
        assert!(verify_artifact_subject(&sha256, &[bundle]).is_ok());
    }

    #[test]
    fn test_verify_artifact_subject_mismatched_digest() {
        let sha256 = Sha256Hash::new(Sha256::digest(b"hello world").into());
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let bundle = make_bundle_with_subjects(&[("test.tar.gz", wrong_hash)]);
        let err = verify_artifact_subject(&sha256, &[bundle]).unwrap_err();
        assert!(
            err.to_string().contains("does not match any subject"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_verify_artifact_subject_multiple_subjects_one_matches() {
        let sha256 = Sha256Hash::new(Sha256::digest(b"my artifact").into());
        let wrong_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let bundle = make_bundle_with_subjects(&[
            ("other.tar.gz", wrong_hash),
            ("correct.tar.gz", &sha256.to_hex()),
        ]);
        assert!(verify_artifact_subject(&sha256, &[bundle]).is_ok());
    }

    #[test]
    fn test_verify_artifact_subject_empty_subjects() {
        let sha256 = Sha256Hash::new(Sha256::digest(b"test").into());
        let bundle = make_bundle_with_subjects(&[]);
        let err = verify_artifact_subject(&sha256, &[bundle]).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not contain any in-toto subjects"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_forged_bundle_with_matching_subject_is_rejected() {
        // A bundle that names the right digest but carries no valid signature
        // must never be accepted, regardless of the configured publishers.
        let artifact = b"hello world";
        let sha256 = Sha256Hash::new(Sha256::digest(artifact).into());
        let bundles = vec![make_bundle_with_subjects(&[(
            "test.tar.gz",
            &sha256.to_hex(),
        )])];
        assert!(verify_artifact_subject(&sha256, &bundles).is_ok());

        for (kind, root) in [
            (
                AttestationKind::PublicGood,
                sigstore_trust_root::SIGSTORE_PRODUCTION_TRUSTED_ROOT,
            ),
            (
                AttestationKind::GitHubRelease,
                sigstore_trust_root::SIGSTORE_GITHUB_TRUSTED_ROOT,
            ),
        ] {
            let trusted_root = TrustedRoot::from_json(root).unwrap();
            let (verified, errors) = verify_bundles(
                &Artifact::from_blob(artifact),
                &bundles,
                kind,
                &trusted_root,
            );
            assert!(verified.is_empty());
            assert_eq!(errors.len(), 1);
        }
    }

    // ---- Offline tests against recorded real-world attestations ----
    //
    // These run in CI and exercise the full cryptographic path (certificate
    // chain, transparency log / timestamp, signature, identity) with the
    // trust roots embedded in sigstore-trust-root.

    const FLASK_SHA: &str = "284c7b8f2f58cb737f0cf1c30fd7eaf0ccfcde196099d24ecede3fc2005aa59e";
    const FLASK_PROVENANCE: &str =
        include_str!("../tests/data/attestations/flask-3.1.1.tar.gz.provenance.json");
    const UV_SHA: &str = "5f0503f896f1209f6114835a7754369c2484331b0caab4c1ecd4579dfd2a31b0";
    const UV_ATTESTATIONS: &str =
        include_str!("../tests/data/attestations/uv-0.12.1-source.tar.gz.github-attestations.json");
    const COSIGN_SHA: &str = "424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa";
    const COSIGN_BUNDLE: &str =
        include_str!("../tests/data/attestations/cosign-v3.1.4-linux-amd64.sigstore.json");

    fn offline_check(
        sha256_hex: &str,
        response: &str,
        kind: AttestationKind,
        cfg: &AttestationVerification,
    ) -> Result<Vec<AttestedIds>, CacheError> {
        let root = match kind {
            AttestationKind::PublicGood => sigstore_trust_root::SIGSTORE_PRODUCTION_TRUSTED_ROOT,
            AttestationKind::GitHubRelease => sigstore_trust_root::SIGSTORE_GITHUB_TRUSTED_ROOT,
        };
        let trusted_root = TrustedRoot::from_json(root).unwrap();
        let digest = sha(sha256_hex);
        check_attestation_response(
            Artifact::from_digest(digest),
            &digest,
            response,
            kind,
            &trusted_root,
            cfg,
        )
    }

    fn no_publishers() -> AttestationVerification {
        AttestationVerification {
            bundle_url: None,
            identity_checks: vec![],
        }
    }

    #[test]
    fn test_offline_pypi_provenance() {
        let kind = AttestationKind::PublicGood;
        let repo = "https://github.com/pallets/flask";
        for cfg in [
            config(repo, None),
            config(repo, Some("refs/tags/3.1.1")),
            config("https://github.com/PALLETS/flask", None),
            no_publishers(),
        ] {
            offline_check(FLASK_SHA, FLASK_PROVENANCE, kind, &cfg)
                .unwrap_or_else(|e| panic!("{cfg:?}: {e}"));
        }

        let err = offline_check(
            FLASK_SHA,
            FLASK_PROVENANCE,
            kind,
            &config("https://github.com/pallets/flask-cors", None),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("identity mismatch"), "{err}");
        assert!(err.contains("publish.yaml@refs/tags/3.1.1"), "{err}");

        let err = offline_check(
            FLASK_SHA,
            FLASK_PROVENANCE,
            kind,
            &config(repo, Some("refs/tags/3.1.0")),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("expected ref: refs/tags/3.1.0"), "{err}");

        // A GitLab publisher with the same path must not match a GitHub issuer.
        let gitlab = IdentityCheck {
            identity: "https://github.com/pallets/flask".to_string(),
            issuer: "https://gitlab.com".to_string(),
            ref_constraint: None,
            repository_id: None,
            owner_id: None,
            environment: None,
        };
        let cfg = AttestationVerification {
            bundle_url: None,
            identity_checks: vec![gitlab],
        };
        assert!(offline_check(FLASK_SHA, FLASK_PROVENANCE, kind, &cfg).is_err());

        let err = offline_check(ABC_SHA, FLASK_PROVENANCE, kind, &config(repo, None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not match any subject"), "{err}");
    }

    /// A single-publisher config that also pins IDs and an environment.
    fn pinned(
        identity: &str,
        repository_id: Option<&str>,
        owner_id: Option<&str>,
        environment: Option<&str>,
    ) -> AttestationVerification {
        let mut cfg = config(identity, None);
        let check = &mut cfg.identity_checks[0];
        check.repository_id = repository_id.map(ToOwned::to_owned);
        check.owner_id = owner_id.map(ToOwned::to_owned);
        check.environment = environment.map(ToOwned::to_owned);
        cfg
    }

    #[test]
    fn test_offline_pinned_ids_pypi_provenance() {
        let kind = AttestationKind::PublicGood;
        let repo = "https://github.com/pallets/flask";
        for cfg in [
            pinned(repo, Some("596892"), None, None),
            pinned(repo, None, Some("16748505"), None),
            pinned(repo, Some("596892"), Some("16748505"), None),
        ] {
            offline_check(FLASK_SHA, FLASK_PROVENANCE, kind, &cfg)
                .unwrap_or_else(|e| panic!("{cfg:?}: {e}"));
        }

        // Right name, wrong repository: what a recreated `pallets/flask` would
        // look like after the original was deleted or renamed.
        for (cfg, expected) in [
            (
                pinned(repo, Some("596893"), Some("16748505"), None),
                "expected repository_id: 596893",
            ),
            (
                pinned(repo, Some("596892"), Some("1"), None),
                "expected owner_id: 1",
            ),
            // The flask certificate carries no deployment environment, so an
            // environment pin fails closed.
            (
                pinned(repo, None, None, Some("publish")),
                "expected environment: publish",
            ),
        ] {
            let err = offline_check(FLASK_SHA, FLASK_PROVENANCE, kind, &cfg)
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "{err}");
            assert!(
                err.contains("repository_id: 596892, owner_id: 16748505)"),
                "{err}"
            );
        }
    }

    #[test]
    fn test_offline_returns_attested_ids() {
        let flask = AttestedIds {
            repository_id: Some("596892".into()),
            owner_id: Some("16748505".into()),
        };
        // One entry per publisher, also when the publisher pins nothing.
        let mut cfg = config("https://github.com/pallets/flask", Some("refs/tags/3.1.1"));
        cfg.identity_checks.push(cfg.identity_checks[0].clone());
        let ids = offline_check(
            FLASK_SHA,
            FLASK_PROVENANCE,
            AttestationKind::PublicGood,
            &cfg,
        )
        .unwrap();
        assert_eq!(ids, [flask.clone(), flask]);

        // GitHub release attestations: from the signed release predicate.
        let ids = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            AttestationKind::GitHubRelease,
            &config("https://github.com/astral-sh/uv", None),
        )
        .unwrap();
        assert_eq!(
            ids,
            [AttestedIds {
                repository_id: Some("699532645".into()),
                owner_id: Some("115962839".into()),
            }]
        );

        let ids = offline_check(
            FLASK_SHA,
            FLASK_PROVENANCE,
            AttestationKind::PublicGood,
            &no_publishers(),
        )
        .unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn test_pin_hint() {
        let pins = Pins {
            repository_id: Some("596892".into()),
            owner_id: Some("16748505".into()),
            environment: None,
        };
        let repo = "https://github.com/pallets/flask";
        assert_eq!(
            pin_hint(&check(repo, None), &pins).unwrap(),
            "`github:pallets(16748505)/flask(596892)`"
        );
        assert_eq!(
            pin_hint(&check(repo, Some("refs/tags/3.1.1")), &pins).unwrap(),
            "`github:pallets(16748505)/flask(596892)` (keep your `@ref`)"
        );
        let mut mapping = check(repo, None);
        mapping.environment = Some("publish".into());
        assert_eq!(
            pin_hint(&mapping, &pins).unwrap(),
            "add `repository_id: 596892` and `owner_id: 16748505`"
        );
        assert!(pin_hint(&check(repo, None), &Pins::default()).is_none());
    }

    #[test]
    fn test_offline_pinned_ids_github_attestations() {
        let repo = "https://github.com/astral-sh/uv";
        let uv_ids = (Some("699532645"), Some("115962839"));

        // Build provenance: IDs and environment from the signing certificate.
        let kind = AttestationKind::PublicGood;
        for cfg in [
            pinned(repo, uv_ids.0, uv_ids.1, None),
            pinned(repo, uv_ids.0, uv_ids.1, Some("release")),
        ] {
            offline_check(UV_SHA, UV_ATTESTATIONS, kind, &cfg)
                .unwrap_or_else(|e| panic!("{cfg:?}: {e}"));
        }
        for cfg in [
            pinned(repo, Some("1"), uv_ids.1, None),
            pinned(repo, uv_ids.0, uv_ids.1, Some("staging")),
        ] {
            let err = offline_check(UV_SHA, UV_ATTESTATIONS, kind, &cfg)
                .unwrap_err()
                .to_string();
            assert!(err.contains("environment: release)"), "{err}");
        }

        // Release attestation: IDs from the signed release predicate.
        let kind = AttestationKind::GitHubRelease;
        offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &pinned(repo, uv_ids.0, uv_ids.1, None),
        )
        .unwrap();
        let err = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &pinned(repo, uv_ids.0, Some("1"), None),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains(
                "pkg:github/astral-sh/uv@0.12.1 (repository_id: 699532645, owner_id: 115962839)"
            ),
            "{err}"
        );
        let err = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &pinned(repo, uv_ids.0, uv_ids.1, Some("release")),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("do not record a deployment environment"),
            "{err}"
        );
    }

    #[test]
    fn test_offline_pypi_provenance_tampered_tlog_is_rejected() {
        // Corrupt the transparency log inclusion proof: verification must
        // fail rather than fall back to skipping the transparency log.
        let mut value: serde_json::Value = serde_json::from_str(FLASK_PROVENANCE).unwrap();
        let entry = &mut value["attestation_bundles"][0]["attestations"][0]["verification_material"]
            ["transparency_entries"][0];
        let root_hash = entry["inclusionProof"]["rootHash"]
            .as_str()
            .unwrap()
            .to_string();
        let mut chars: Vec<char> = root_hash.chars().collect();
        chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
        entry["inclusionProof"]["rootHash"] = chars.into_iter().collect::<String>().into();

        let err = offline_check(
            FLASK_SHA,
            &value.to_string(),
            AttestationKind::PublicGood,
            &no_publishers(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("signature verification"), "{err}");
    }

    #[test]
    fn test_offline_github_release_attestation() {
        let kind = AttestationKind::GitHubRelease;
        let repo = "https://github.com/astral-sh/uv";
        for cfg in [
            config(repo, None),
            config(repo, Some("refs/tags/0.12.1")),
            no_publishers(),
        ] {
            offline_check(UV_SHA, UV_ATTESTATIONS, kind, &cfg)
                .unwrap_or_else(|e| panic!("{cfg:?}: {e}"));
        }

        let err = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &config("https://github.com/astral-sh/ruff", None),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("pkg:github/astral-sh/uv@0.12.1"), "{err}");

        let err = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &config(repo, Some("refs/heads/main")),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("expected ref: refs/heads/main"), "{err}");
    }

    /// Keep only the bundles of the recorded GitHub response whose statement
    /// has the given predicate type prefix.
    fn uv_attestations_with_predicate(prefix: &str) -> String {
        let mut value: serde_json::Value = serde_json::from_str(UV_ATTESTATIONS).unwrap();
        value["attestations"]
            .as_array_mut()
            .unwrap()
            .retain(|attestation| {
                let bundle_json = serde_json::to_string(&attestation["bundle"]).unwrap();
                let bundle = sigstore_types::Bundle::from_json(&bundle_json).unwrap();
                bundle_statement(&bundle).unwrap()["predicateType"]
                    .as_str()
                    .unwrap()
                    .starts_with(prefix)
            });
        assert_eq!(value["attestations"].as_array().unwrap().len(), 1);
        value.to_string()
    }

    #[test]
    fn test_offline_github_release_attestation_requires_github_trust_domain() {
        // A release attestation must not verify against the public-good root:
        // the trust domain follows from the requested URL, not the response.
        let err = offline_check(
            UV_SHA,
            &uv_attestations_with_predicate(RELEASE_PREDICATE_PREFIX),
            AttestationKind::PublicGood,
            &no_publishers(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("signature verification"), "{err}");

        // Conversely, build provenance is not accepted as a release attestation.
        let err = offline_check(
            UV_SHA,
            &uv_attestations_with_predicate("https://slsa.dev/provenance/"),
            AttestationKind::GitHubRelease,
            &no_publishers(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("not a release attestation"), "{err}");
    }

    #[test]
    fn test_offline_github_build_provenance() {
        // `bundle_url` pointing at the attestations API without the release
        // filter verifies SLSA build provenance against the workflow identity.
        // uv releases via `workflow_dispatch` on main, so the workflow ref in
        // the certificate is the branch, not the tag.
        let kind = AttestationKind::PublicGood;
        let repo = "https://github.com/astral-sh/uv";
        for cfg in [config(repo, None), config(repo, Some("refs/heads/main"))] {
            offline_check(UV_SHA, UV_ATTESTATIONS, kind, &cfg)
                .unwrap_or_else(|e| panic!("{cfg:?}: {e}"));
        }
        let err = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &config("https://github.com/astral-sh/ruff", None),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("https://github.com/astral-sh/uv/"), "{err}");

        let err = offline_check(
            UV_SHA,
            UV_ATTESTATIONS,
            kind,
            &config(repo, Some("refs/tags/0.12.1")),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("release.yml@refs/heads/main"), "{err}");
    }

    #[test]
    fn test_offline_message_signature_bundle() {
        // cosign signs its release binaries keylessly with a Google service
        // account; the bundle is a message signature, not an in-toto statement.
        let kind = AttestationKind::PublicGood;
        offline_check(COSIGN_SHA, COSIGN_BUNDLE, kind, &no_publishers()).unwrap();

        let err = offline_check(
            COSIGN_SHA,
            COSIGN_BUNDLE,
            kind,
            &config("https://github.com/sigstore/cosign", None),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("keyless@projectsigstore.iam.gserviceaccount.com"),
            "{err}"
        );

        let err = offline_check(ABC_SHA, COSIGN_BUNDLE, kind, &no_publishers())
            .unwrap_err()
            .to_string();
        assert!(err.contains("signature verification"), "{err}");
    }

    // ---- Network tests (run with `cargo test -- --ignored`) ----

    async fn download_to_temp(client: &BaseClient, url: &url::Url) -> tempfile::NamedTempFile {
        let bytes = client
            .for_host(url)
            .client()
            .get(url.clone())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs_err::tokio::write(temp.path(), bytes).await.unwrap();
        temp
    }

    fn config(publisher: &str, ref_constraint: Option<&str>) -> AttestationVerification {
        AttestationVerification {
            bundle_url: None,
            identity_checks: vec![check(publisher, ref_constraint)],
        }
    }

    fn flip_byte(path: &Path) {
        let mut bytes = fs_err::read(path).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
        fs_err::write(path, bytes).unwrap();
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn test_uv_github_release_attestation() {
        let source_url = url::Url::parse(
            "https://github.com/astral-sh/uv/releases/download/0.12.1/source.tar.gz",
        )
        .unwrap();
        let client = BaseClient::builder().timeout(300).build();
        let temp = download_to_temp(&client, &source_url).await;
        let repo = "https://github.com/astral-sh/uv";

        for cfg in [
            config(repo, None),
            config(repo, Some("refs/tags/0.12.1")),
            AttestationVerification {
                bundle_url: None,
                identity_checks: vec![],
            },
        ] {
            verify_attestation(&client, temp.path(), &source_url, &cfg)
                .await
                .unwrap();
        }

        let error = verify_attestation(
            &client,
            temp.path(),
            &source_url,
            &config("https://github.com/astral-sh/xv", None),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("expected repository identity: https://github.com/astral-sh/xv"));
        assert!(error.contains("pkg:github/astral-sh/uv@0.12.1"));
        assert!(error.contains("signing certificate identity: https://dotcom.releases.github.com"));

        let error = verify_attestation(
            &client,
            temp.path(),
            &source_url,
            &config(repo, Some("refs/tags/0.12.0")),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("expected ref: refs/tags/0.12.0"), "{error}");

        // A modified artifact has no attestation at all.
        flip_byte(temp.path());
        let error = verify_attestation(&client, temp.path(), &source_url, &config(repo, None))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("no release attestation"), "{error}");
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn test_pypi_provenance_attestations() {
        let client = BaseClient::builder().timeout(300).build();
        let cases = [
            (
                "https://files.pythonhosted.org/packages/source/f/flask/flask-3.1.1.tar.gz",
                "https://github.com/pallets/flask",
                "refs/tags/3.1.1",
            ),
            (
                "https://files.pythonhosted.org/packages/py3/f/flask/flask-3.1.1-py3-none-any.whl",
                "https://github.com/pallets/flask",
                "refs/tags/3.1.1",
            ),
            (
                "https://files.pythonhosted.org/packages/source/u/urllib3/urllib3-2.5.0.tar.gz",
                "https://github.com/urllib3/urllib3",
                "refs/tags/2.5.0",
            ),
            (
                "https://files.pythonhosted.org/packages/source/s/sigstore/sigstore-3.6.5.tar.gz",
                "https://github.com/sigstore/sigstore-python",
                "refs/tags/v3.6.5",
            ),
        ];

        for (source, publisher, git_ref) in cases {
            let source_url = url::Url::parse(source).unwrap();
            let temp = download_to_temp(&client, &source_url).await;

            // Fully verified, including transparency log inclusion; with and
            // without a ref constraint, and without any publishers.
            for cfg in [
                config(publisher, None),
                config(publisher, Some(git_ref)),
                AttestationVerification {
                    bundle_url: None,
                    identity_checks: vec![],
                },
            ] {
                verify_attestation(&client, temp.path(), &source_url, &cfg)
                    .await
                    .unwrap_or_else(|e| panic!("{source} ({cfg:?}): {e}"));
            }

            let error = verify_attestation(
                &client,
                temp.path(),
                &source_url,
                &config("https://github.com/attacker/project", None),
            )
            .await
            .unwrap_err()
            .to_string();
            assert!(error.contains("identity mismatch"), "{source}: {error}");

            let error = verify_attestation(
                &client,
                temp.path(),
                &source_url,
                &config(publisher, Some("refs/tags/0.0.0")),
            )
            .await
            .unwrap_err()
            .to_string();
            assert!(error.contains("expected ref"), "{source}: {error}");

            // Flip a byte: the digest no longer matches the attestation.
            flip_byte(temp.path());
            let error =
                verify_attestation(&client, temp.path(), &source_url, &config(publisher, None))
                    .await
                    .unwrap_err()
                    .to_string();
            assert!(
                error.contains("does not match any subject"),
                "{source}: {error}"
            );
        }
    }
}
