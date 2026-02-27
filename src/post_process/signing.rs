//! Code signing post-processing step for macOS and Windows binaries.
//!
//! This module handles signing native binaries (executables, shared libraries)
//! using platform-specific tools:
//! - macOS: `codesign` with a real signing identity
//! - Windows: `signtool` with a certificate file
//!
//! Signing happens AFTER relinking (which may invalidate existing signatures)
//! and BEFORE packaging, so the archive contains properly signed binaries.

use std::path::{Path, PathBuf};

use rattler_build_recipe::stage1::build::{
    MacOsSigning, Signing, WindowsSigning, WindowsSigningMethod,
};

#[cfg(test)]
use rattler_build_recipe::stage1::build::{AzureTrustedSigningConfig, SigntoolConfig};
use rattler_conda_types::Platform;
use thiserror::Error;

use crate::{
    macos::link::Dylib,
    metadata::Output,
    packaging::{TempFiles, contains_prefix_binary},
    post_process::relink::{RelinkError, Relinker},
    system_tools::{SystemTools, Tool, ToolError},
    windows::link::Dll,
};

/// Errors that can occur during code signing
#[derive(Error, Debug)]
pub enum SigningError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// macOS codesign failed
    #[error("macOS codesign failed for {path}: {message}")]
    MacOsCodesignFailed { path: PathBuf, message: String },

    /// Windows signtool failed
    #[error("Windows signtool failed for {path}: {message}")]
    WindowsSigntoolFailed { path: PathBuf, message: String },

    /// Signature verification failed
    #[error("Signature verification failed for {path}: {message}")]
    VerificationFailed { path: PathBuf, message: String },

    /// Signed binary contains the build prefix, which would be corrupted at install time
    #[error(
        "Signed binary '{path}' contains the build prefix. \
         Prefix replacement at install time will destroy the signature. \
         Either ensure the binary doesn't embed the prefix path, or add the file \
         to build.prefix_detection.ignore"
    )]
    SignedBinaryContainsPrefix { path: PathBuf },

    /// System tool not found
    #[error("System tool error: {0}")]
    ToolError(#[from] ToolError),

    /// Relink error (for file type detection)
    #[error("Relink error: {0}")]
    RelinkError(#[from] RelinkError),
}

/// Sign a single macOS binary using `codesign`
fn sign_macos_binary(
    path: &Path,
    config: &MacOsSigning,
    system_tools: &SystemTools,
) -> Result<(), SigningError> {
    let codesign = system_tools
        .find_tool(Tool::Codesign)
        .map_err(|e| ToolError::ToolNotFound(Tool::Codesign, e))?;

    let mut cmd = std::process::Command::new(&codesign);
    cmd.arg("--force");
    cmd.arg("--sign");
    cmd.arg(&config.identity);

    if let Some(keychain) = &config.keychain {
        cmd.arg("--keychain");
        cmd.arg(keychain);
    }

    if let Some(entitlements) = &config.entitlements {
        cmd.arg("--entitlements");
        cmd.arg(entitlements);
    }

    for option in &config.options {
        cmd.arg("--options");
        cmd.arg(option);
    }

    // Preserve existing metadata when re-signing
    let is_system_codesign = codesign.starts_with("/usr/bin/");
    if is_system_codesign {
        cmd.arg("--preserve-metadata=entitlements,requirements");
    }

    // Add --timestamp by default for non-adhoc signatures
    if config.identity != "-" && !config.options.iter().any(|o| o == "timestamp") {
        cmd.arg("--timestamp");
    }

    cmd.arg(path);

    tracing::debug!("Running codesign: {:?}", cmd);

    let output = cmd
        .output()
        .map_err(|e| SigningError::MacOsCodesignFailed {
            path: path.to_path_buf(),
            message: format!("Failed to execute codesign: {}", e),
        })?;

    if !output.status.success() {
        return Err(SigningError::MacOsCodesignFailed {
            path: path.to_path_buf(),
            message: format!(
                "codesign exited with status {}.\n  stdout: {}\n  stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    tracing::info!("Signed (macOS): {}", path.display());
    Ok(())
}

/// Verify a macOS signature using `codesign --verify`
fn verify_macos_signature(path: &Path, system_tools: &SystemTools) -> Result<(), SigningError> {
    let codesign = system_tools
        .find_tool(Tool::Codesign)
        .map_err(|e| ToolError::ToolNotFound(Tool::Codesign, e))?;

    let output = std::process::Command::new(codesign)
        .args(["--verify", "--deep", "--strict"])
        .arg(path)
        .output()
        .map_err(|e| SigningError::VerificationFailed {
            path: path.to_path_buf(),
            message: format!("Failed to execute codesign verify: {}", e),
        })?;

    if !output.status.success() {
        return Err(SigningError::VerificationFailed {
            path: path.to_path_buf(),
            message: format!(
                "Verification failed.\n  stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}

/// Sign a single Windows binary.
///
/// Dispatches to either `signtool` (local certificate) or Azure Trusted Signing
/// based on the configuration.
fn sign_windows_binary(
    path: &Path,
    config: &WindowsSigning,
    system_tools: &SystemTools,
) -> Result<(), SigningError> {
    let method = config
        .method()
        .map_err(|e| SigningError::WindowsSigntoolFailed {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    match method {
        WindowsSigningMethod::Signtool {
            certificate_file,
            certificate_password_env,
        } => sign_windows_signtool(
            path,
            certificate_file,
            certificate_password_env,
            config,
            system_tools,
        ),
        WindowsSigningMethod::AzureTrustedSigning {
            endpoint,
            account_name,
            certificate_profile,
        } => sign_windows_azure(path, endpoint, account_name, certificate_profile, config),
    }
}

/// Sign a Windows binary using local `signtool` with a certificate file.
fn sign_windows_signtool(
    path: &Path,
    certificate_file: &str,
    certificate_password_env: Option<&str>,
    config: &WindowsSigning,
    system_tools: &SystemTools,
) -> Result<(), SigningError> {
    let signtool = system_tools
        .find_tool(Tool::Signtool)
        .map_err(|e| ToolError::ToolNotFound(Tool::Signtool, e))?;

    let mut cmd = std::process::Command::new(signtool);
    cmd.arg("sign");

    cmd.arg("/f");
    cmd.arg(certificate_file);

    if let Some(env_var) = certificate_password_env {
        let password = std::env::var(env_var).map_err(|_| SigningError::WindowsSigntoolFailed {
            path: path.to_path_buf(),
            message: format!(
                "Environment variable '{}' not set (required for certificate password)",
                env_var
            ),
        })?;
        cmd.arg("/p");
        cmd.arg(password);
    }

    cmd.arg("/fd");
    cmd.arg(&config.digest_algorithm);

    if let Some(timestamp_url) = &config.timestamp_url {
        cmd.arg("/tr");
        cmd.arg(timestamp_url);
        cmd.arg("/td");
        cmd.arg(&config.digest_algorithm);
    }

    cmd.arg(path);

    tracing::debug!("Running signtool: {:?}", cmd);

    let output = cmd
        .output()
        .map_err(|e| SigningError::WindowsSigntoolFailed {
            path: path.to_path_buf(),
            message: format!("Failed to execute signtool: {}", e),
        })?;

    if !output.status.success() {
        return Err(SigningError::WindowsSigntoolFailed {
            path: path.to_path_buf(),
            message: format!(
                "signtool exited with status {}.\n  stdout: {}\n  stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    tracing::info!("Signed (Windows/signtool): {}", path.display());
    Ok(())
}

/// Sign a Windows binary using Azure Trusted Signing.
///
/// This invokes the Azure Code Signing tool (`azure-code-signing`) which must
/// be available on PATH. Authentication is handled via Azure CLI (`az login`),
/// which should be performed before the build (e.g., via OIDC in GitHub Actions).
fn sign_windows_azure(
    path: &Path,
    endpoint: &str,
    account_name: &str,
    certificate_profile: &str,
    config: &WindowsSigning,
) -> Result<(), SigningError> {
    // The Azure Trusted Signing CLI tool. In CI, this is typically installed
    // by the azure/trusted-signing-action. For direct invocation, we use
    // the `signtool` dlib approach or the standalone Azure Code Signing tool.
    // The tool is invoked as:
    //   signtool sign /v /fd SHA256 /tr <timestamp> /td SHA256
    //     /dlib "Azure.CodeSigning.Dlib.dll"
    //     /dmdf <metadata.json>
    //     <file>
    //
    // However, for simplicity and CI compatibility, we generate the metadata
    // JSON inline and use the AzureCodeSigning tool directly.

    // Build metadata JSON for Azure Trusted Signing
    let metadata = serde_json::json!({
        "Endpoint": endpoint,
        "CodeSigningAccountName": account_name,
        "CertificateProfileName": certificate_profile,
    });

    // Write metadata to a temp file
    let temp_dir = tempfile::tempdir().map_err(SigningError::Io)?;
    let metadata_path = temp_dir.path().join("signing-metadata.json");
    fs_err::write(&metadata_path, metadata.to_string()).map_err(SigningError::Io)?;

    // Try the Azure Code Signing tool first
    let azure_tool = which::which("AzureCodeSigning")
        .or_else(|_| which::which("azure-code-signing"))
        .or_else(|_| which::which("signtool"));

    let tool_path = azure_tool.map_err(|e| SigningError::WindowsSigntoolFailed {
        path: path.to_path_buf(),
        message: format!(
            "Could not find Azure Code Signing tool or signtool: {}. \
             Ensure the Azure Trusted Signing action has been run or the tool is installed.",
            e
        ),
    })?;

    let mut cmd = std::process::Command::new(&tool_path);

    let tool_name = tool_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    if tool_name.eq_ignore_ascii_case("signtool") {
        // Use signtool with Azure Code Signing DLib
        cmd.arg("sign");
        cmd.arg("/v");
        cmd.arg("/fd");
        cmd.arg(&config.digest_algorithm);

        if let Some(timestamp_url) = &config.timestamp_url {
            cmd.arg("/tr");
            cmd.arg(timestamp_url);
            cmd.arg("/td");
            cmd.arg(&config.digest_algorithm);
        }

        // Point to the Azure Code Signing DLib and metadata
        cmd.arg("/dlib");
        cmd.arg("Azure.CodeSigning.Dlib.dll");
        cmd.arg("/dmdf");
        cmd.arg(&metadata_path);
        cmd.arg(path);
    } else {
        // AzureCodeSigning standalone tool
        cmd.arg("sign");
        cmd.arg("-mdf");
        cmd.arg(&metadata_path);
        cmd.arg("-fd");
        cmd.arg(&config.digest_algorithm);

        if let Some(timestamp_url) = &config.timestamp_url {
            cmd.arg("-tr");
            cmd.arg(timestamp_url);
            cmd.arg("-td");
            cmd.arg(&config.digest_algorithm);
        }

        cmd.arg(path);
    }

    tracing::debug!("Running Azure Trusted Signing: {:?}", cmd);

    let output = cmd
        .output()
        .map_err(|e| SigningError::WindowsSigntoolFailed {
            path: path.to_path_buf(),
            message: format!("Failed to execute Azure Trusted Signing: {}", e),
        })?;

    if !output.status.success() {
        return Err(SigningError::WindowsSigntoolFailed {
            path: path.to_path_buf(),
            message: format!(
                "Azure Trusted Signing exited with status {}.\n  stdout: {}\n  stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    tracing::info!("Signed (Windows/Azure Trusted Signing): {}", path.display());
    Ok(())
}

/// Verify a Windows signature using `signtool verify`
fn verify_windows_signature(path: &Path, system_tools: &SystemTools) -> Result<(), SigningError> {
    let signtool = system_tools
        .find_tool(Tool::Signtool)
        .map_err(|e| ToolError::ToolNotFound(Tool::Signtool, e))?;

    let output = std::process::Command::new(signtool)
        .args(["verify", "/pa"])
        .arg(path)
        .output()
        .map_err(|e| SigningError::VerificationFailed {
            path: path.to_path_buf(),
            message: format!("Failed to execute signtool verify: {}", e),
        })?;

    if !output.status.success() {
        return Err(SigningError::VerificationFailed {
            path: path.to_path_buf(),
            message: format!(
                "Verification failed.\n  stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    Ok(())
}

/// Check if a file is a signable binary for the given platform.
fn is_signable_binary(platform: Platform, path: &Path) -> bool {
    if path.is_symlink() || path.is_dir() {
        return false;
    }

    if platform.is_osx() {
        Dylib::test_file(path).unwrap_or(false)
    } else if platform.is_windows() {
        Dll::try_new(path).ok().flatten().is_some()
    } else {
        false
    }
}

/// Sign all signable binaries in the package.
///
/// This function:
/// 1. Determines which platform-specific signing config applies
/// 2. Iterates over all files in the temp directory
/// 3. Signs each signable binary (Mach-O on macOS, PE on Windows)
/// 4. Verifies signatures after signing
/// 5. Checks that signed binaries don't contain the build prefix
///
/// Returns the list of signed file paths.
pub fn sign_binaries(
    temp_files: &TempFiles,
    output: &Output,
) -> Result<Vec<PathBuf>, SigningError> {
    let signing = &output.recipe.build().signing;
    let target_platform = output.build_configuration.target_platform;
    let system_tools = &output.system_tools;

    // Determine which signing config applies for this target platform
    let (macos_config, windows_config) = get_platform_signing_config(signing, target_platform);

    if macos_config.is_none() && windows_config.is_none() {
        return Ok(Vec::new());
    }

    tracing::info!("Signing binaries...");

    let tmp_prefix = temp_files.temp_dir.path();
    let mut signed_files = Vec::new();

    for file_path in &temp_files.files {
        if !is_signable_binary(target_platform, file_path) {
            continue;
        }

        let rel_path = file_path.strip_prefix(tmp_prefix).unwrap_or(file_path);

        if let Some(config) = macos_config {
            sign_macos_binary(file_path, config, system_tools)?;
            verify_macos_signature(file_path, system_tools)?;
            tracing::debug!("Verified signature: {}", rel_path.display());
        } else if let Some(config) = windows_config {
            sign_windows_binary(file_path, config, system_tools)?;
            verify_windows_signature(file_path, system_tools)?;
            tracing::debug!("Verified signature: {}", rel_path.display());
        }

        signed_files.push(file_path.clone());
    }

    if !signed_files.is_empty() {
        tracing::info!("Signed {} binaries", signed_files.len());
    }

    Ok(signed_files)
}

/// Check that signed binaries don't contain the build prefix.
///
/// If a signed binary contains the build prefix, conda's prefix replacement
/// at install time would modify the binary content and destroy the signature.
pub fn check_signed_binaries_no_prefix(
    signed_files: &[PathBuf],
    output: &Output,
) -> Result<(), SigningError> {
    if signed_files.is_empty() {
        return Ok(());
    }

    let prefix = &output.build_configuration.directories.host_prefix;

    for file_path in signed_files {
        match contains_prefix_binary(file_path, prefix) {
            Ok(true) => {
                let rel_path = file_path
                    .strip_prefix(output.build_configuration.directories.host_prefix.as_path())
                    .unwrap_or(file_path);
                return Err(SigningError::SignedBinaryContainsPrefix {
                    path: rel_path.to_path_buf(),
                });
            }
            Ok(false) => {}
            Err(_) => {
                // If we can't check, skip - the file might not be accessible
                tracing::warn!(
                    "Could not check prefix in signed binary: {}",
                    file_path.display()
                );
            }
        }
    }

    Ok(())
}

/// Determine which signing config applies for the target platform.
fn get_platform_signing_config(
    signing: &Signing,
    target_platform: Platform,
) -> (Option<&MacOsSigning>, Option<&WindowsSigning>) {
    if target_platform.is_osx() {
        (signing.macos.as_ref(), None)
    } else if target_platform.is_windows() {
        (None, signing.windows.as_ref())
    } else {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_windows_signtool() -> WindowsSigning {
        WindowsSigning {
            signtool: Some(SigntoolConfig {
                certificate_file: "cert.pfx".to_string(),
                certificate_password_env: None,
            }),
            azure_trusted_signing: None,
            timestamp_url: None,
            digest_algorithm: "sha256".to_string(),
        }
    }

    fn make_windows_azure() -> WindowsSigning {
        WindowsSigning {
            signtool: None,
            azure_trusted_signing: Some(AzureTrustedSigningConfig {
                endpoint: "https://wus2.codesigning.azure.net".to_string(),
                account_name: "my-account".to_string(),
                certificate_profile: "my-profile".to_string(),
            }),
            timestamp_url: Some("http://timestamp.acs.microsoft.com".to_string()),
            digest_algorithm: "sha256".to_string(),
        }
    }

    #[test]
    fn test_get_platform_signing_config_macos() {
        let signing = Signing {
            macos: Some(MacOsSigning {
                identity: "-".to_string(),
                keychain: None,
                entitlements: None,
                options: vec![],
            }),
            windows: Some(make_windows_signtool()),
        };

        let (macos, windows) = get_platform_signing_config(&signing, Platform::OsxArm64);
        assert!(macos.is_some());
        assert!(windows.is_none());

        let (macos, windows) = get_platform_signing_config(&signing, Platform::Osx64);
        assert!(macos.is_some());
        assert!(windows.is_none());
    }

    #[test]
    fn test_get_platform_signing_config_windows() {
        let signing = Signing {
            macos: Some(MacOsSigning {
                identity: "-".to_string(),
                keychain: None,
                entitlements: None,
                options: vec![],
            }),
            windows: Some(make_windows_signtool()),
        };

        let (macos, windows) = get_platform_signing_config(&signing, Platform::Win64);
        assert!(macos.is_none());
        assert!(windows.is_some());
    }

    #[test]
    fn test_get_platform_signing_config_linux() {
        let signing = Signing {
            macos: Some(MacOsSigning {
                identity: "-".to_string(),
                keychain: None,
                entitlements: None,
                options: vec![],
            }),
            windows: Some(make_windows_signtool()),
        };

        let (macos, windows) = get_platform_signing_config(&signing, Platform::Linux64);
        assert!(macos.is_none());
        assert!(windows.is_none());
    }

    #[test]
    fn test_get_platform_signing_config_no_signing() {
        let signing = Signing::default();

        let (macos, windows) = get_platform_signing_config(&signing, Platform::OsxArm64);
        assert!(macos.is_none());
        assert!(windows.is_none());
    }

    #[test]
    fn test_signing_default_is_empty() {
        let signing = Signing::default();
        assert!(signing.is_default());
        assert!(signing.macos.is_none());
        assert!(signing.windows.is_none());
    }

    #[test]
    fn test_windows_signing_method_signtool() {
        let config = make_windows_signtool();
        let method = config.method().unwrap();
        assert!(matches!(method, WindowsSigningMethod::Signtool { .. }));
    }

    #[test]
    fn test_windows_signing_method_azure() {
        let config = make_windows_azure();
        let method = config.method().unwrap();
        assert!(matches!(
            method,
            WindowsSigningMethod::AzureTrustedSigning { .. }
        ));
    }

    #[test]
    fn test_windows_signing_method_both_errors() {
        let config = WindowsSigning {
            signtool: Some(SigntoolConfig {
                certificate_file: "cert.pfx".to_string(),
                certificate_password_env: None,
            }),
            azure_trusted_signing: Some(AzureTrustedSigningConfig {
                endpoint: "https://endpoint".to_string(),
                account_name: "account".to_string(),
                certificate_profile: "profile".to_string(),
            }),
            timestamp_url: None,
            digest_algorithm: "sha256".to_string(),
        };
        assert!(config.method().is_err());
    }

    #[test]
    fn test_windows_signing_method_neither_errors() {
        let config = WindowsSigning {
            signtool: None,
            azure_trusted_signing: None,
            timestamp_url: None,
            digest_algorithm: "sha256".to_string(),
        };
        assert!(config.method().is_err());
    }
}
