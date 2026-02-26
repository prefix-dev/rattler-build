use std::fmt::Display;

use itertools::Itertools as _;
use rattler_conda_types::{NoArchType, package::EntryPoint};
use serde::{Deserialize, Serialize};

use crate::stage0::types::{ConditionalList, IncludeExclude, Item, Script, Value};

/// Variant key usage configuration
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct VariantKeyUsage {
    /// Variant keys to use
    #[serde(default)]
    pub use_keys: ConditionalList<String>,

    /// Variant keys to ignore
    #[serde(default)]
    pub ignore_keys: ConditionalList<String>,

    /// Down-prioritize variant by setting priority to a negative value
    pub down_prioritize_variant: Option<Value<i32>>,
}

/// Stage0 Build configuration - contains templates and conditionals
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Build {
    /// Build number (increments with each rebuild)
    /// None means inherit from top-level, Some(n) means use n (even if n is 0)
    #[serde(default)]
    pub number: Option<Value<u64>>,

    /// Build string (usually auto-generated from variant hash)
    pub string: Option<Value<String>>,

    /// Build script - contains script content, interpreter, environment variables, etc.
    /// Default is `build.sh` on Unix, `build.bat` on Windows
    #[serde(default)]
    pub script: Script,

    /// Noarch type - python or generic
    pub noarch: Option<Value<NoArchType>>,

    /// Python-specific configuration
    #[serde(default)]
    pub python: PythonBuild,

    /// Skip build on certain conditions (can be a boolean expression or list of platforms)
    #[serde(default)]
    pub skip: ConditionalList<String>,

    /// Always copy these files (even if they're already in the host prefix)
    #[serde(default)]
    pub always_copy_files: ConditionalList<String>,

    /// Always include these files in the package
    #[serde(default)]
    pub always_include_files: ConditionalList<String>,

    /// Merge build and host environments
    #[serde(default)]
    pub merge_build_and_host_envs: Value<bool>,

    /// Files to include/exclude in the package (glob patterns or include/exclude mapping)
    #[serde(default)]
    pub files: IncludeExclude,

    /// Dynamic linking configuration
    #[serde(default)]
    pub dynamic_linking: DynamicLinking,

    /// Variant key usage configuration
    #[serde(default)]
    pub variant: VariantKeyUsage,

    /// Prefix detection configuration
    #[serde(default)]
    pub prefix_detection: PrefixDetection,

    /// Post-processing operations
    #[serde(default)]
    pub post_process: ConditionalList<PostProcess>,

    /// Code signing configuration
    #[serde(default)]
    pub signing: Signing,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            number: None,
            string: None,
            script: Script::default(),
            noarch: None,
            python: PythonBuild::default(),
            skip: ConditionalList::default(),
            always_copy_files: ConditionalList::default(),
            always_include_files: ConditionalList::default(),
            merge_build_and_host_envs: Value::new_concrete(false, None),
            files: IncludeExclude::default(),
            dynamic_linking: DynamicLinking::default(),
            variant: VariantKeyUsage::default(),
            prefix_detection: PrefixDetection::default(),
            post_process: ConditionalList::default(),
            signing: Signing::default(),
        }
    }
}

/// Binary relocation configuration - can be a boolean or list of glob patterns
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum BinaryRelocation {
    /// Boolean value (true = relocate all, false = relocate none)
    Boolean(Value<bool>),
    /// List of glob patterns to relocate
    Patterns(ConditionalList<String>),
}

impl Default for BinaryRelocation {
    fn default() -> Self {
        Self::Boolean(Value::new_concrete(true, None))
    }
}

/// Dynamic linking configuration for shared libraries
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct DynamicLinking {
    /// RPaths to use (Linux/macOS only)
    #[serde(default)]
    pub rpaths: ConditionalList<String>,

    /// Binary relocation settings
    /// - Boolean: true = relocate all (default), false = relocate none
    /// - Patterns: list of glob patterns to relocate
    #[serde(default)]
    pub binary_relocation: BinaryRelocation,

    /// Allow these missing DSOs (glob patterns)
    #[serde(default)]
    pub missing_dso_allowlist: ConditionalList<String>,

    /// Allow rpath to point to these locations
    #[serde(default)]
    pub rpath_allowlist: ConditionalList<String>,

    /// What to do when detecting overdepending
    #[serde(default)]
    pub overdepending_behavior: Option<Value<String>>,

    /// What to do when detecting overlinking
    #[serde(default)]
    pub overlinking_behavior: Option<Value<String>>,
}

/// Force file type configuration for prefix detection
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct ForceFileType {
    /// Force these files to be treated as text files
    #[serde(default)]
    pub text: ConditionalList<String>,
    /// Force these files to be treated as binary files
    #[serde(default)]
    pub binary: ConditionalList<String>,
}

/// Prefix detection configuration - can be All(bool) or Patterns(list)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum PrefixIgnore {
    /// Boolean value (true = ignore all, false = ignore none)
    Boolean(Value<bool>),
    /// List of glob patterns to ignore
    Patterns(ConditionalList<String>),
}

impl Default for PrefixIgnore {
    fn default() -> Self {
        Self::Boolean(Value::new_concrete(false, None))
    }
}

/// Prefix detection configuration
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct PrefixDetection {
    /// Force file type detection
    #[serde(default)]
    pub force_file_type: ForceFileType,
    /// Files to ignore for prefix replacement
    #[serde(default)]
    pub ignore: PrefixIgnore,
    /// Ignore binary files for prefix replacement (Unix only)
    #[serde(default)]
    pub ignore_binary_files: Value<bool>,
}

/// Code signing configuration for native binaries
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct Signing {
    /// macOS code signing configuration
    #[serde(default)]
    pub macos: Option<MacOsSigning>,
    /// Windows code signing configuration
    #[serde(default)]
    pub windows: Option<WindowsSigning>,
}

/// macOS code signing configuration using `codesign`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct MacOsSigning {
    /// Signing identity (e.g., "Developer ID Application: Company (TEAMID)")
    /// Use "-" for ad-hoc signing
    pub identity: Value<String>,
    /// Path to the keychain containing the signing certificate
    #[serde(default)]
    pub keychain: Option<Value<String>>,
    /// Entitlements plist file path
    #[serde(default)]
    pub entitlements: Option<Value<String>>,
    /// Additional codesign options (e.g., "runtime" for hardened runtime)
    #[serde(default)]
    pub options: ConditionalList<String>,
}

/// Windows code signing configuration.
///
/// Supports two signing methods (exactly one must be configured):
/// 1. **Local certificate** (`signtool`): Configure the `signtool` sub-object.
/// 2. **Azure Trusted Signing**: Configure the `azure_trusted_signing` sub-object.
///
/// Shared settings (`timestamp_url`, `digest_algorithm`) live at the top level.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct WindowsSigning {
    /// Local certificate signing via `signtool`
    #[serde(default)]
    pub signtool: Option<SigntoolConfig>,
    /// Azure Trusted Signing configuration
    #[serde(default)]
    pub azure_trusted_signing: Option<AzureTrustedSigningConfig>,

    // --- Shared settings ---
    /// RFC 3161 timestamp server URL
    #[serde(default)]
    pub timestamp_url: Option<Value<String>>,
    /// Digest algorithm (default: sha256)
    #[serde(default)]
    pub digest_algorithm: Option<Value<String>>,
}

/// Local certificate signing configuration for `signtool`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SigntoolConfig {
    /// Path to the certificate file (.pfx / .p12)
    pub certificate_file: Value<String>,
    /// Name of the environment variable containing the certificate password.
    /// The password is read from this env var at build time to avoid leaking
    /// secrets into the rendered recipe.
    #[serde(default)]
    pub certificate_password_env: Option<Value<String>>,
}

/// Azure Trusted Signing configuration
///
/// Requires `az login` (OIDC) for authentication.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AzureTrustedSigningConfig {
    /// Azure Trusted Signing endpoint URL
    pub endpoint: Value<String>,
    /// Azure Trusted Signing account name
    pub account_name: Value<String>,
    /// Azure Trusted Signing certificate profile name
    pub certificate_profile: Value<String>,
}

/// Post-processing operations using regex replacements
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PostProcess {
    /// Files to apply post-processing to
    pub files: ConditionalList<String>,
    /// Regular expression pattern to match
    pub regex: Value<String>,
    /// Replacement string
    pub replacement: Value<String>,
}

/// Python-specific build configuration
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PythonBuild {
    /// Python entry points (executable_name = module:function)
    #[serde(default)]
    pub entry_points: ConditionalList<EntryPoint>,

    /// Skip pyc compilation for these files (glob patterns)
    /// Only relevant for non-noarch Python packages
    #[serde(default)]
    pub skip_pyc_compilation: ConditionalList<String>,

    /// Use Python.app on macOS for GUI applications
    #[serde(default)]
    pub use_python_app_entrypoint: Value<bool>,

    /// Whether the package is Python version independent
    /// This is used for abi3 packages that are not tied to a specific Python version
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_independent: Option<Value<bool>>,

    /// The relative site-packages path that a Python build exports for other packages to use
    /// This setting only makes sense for the `python` package itself
    pub site_packages_path: Option<Value<String>>,
}

// Manual PartialEq implementation since EntryPoint doesn't implement PartialEq
impl PartialEq for PythonBuild {
    fn eq(&self, other: &Self) -> bool {
        // Compare all fields except entry_points which can't be compared
        // We compare the length and assume they're equal if lengths match
        self.entry_points.len() == other.entry_points.len()
            && self.skip_pyc_compilation == other.skip_pyc_compilation
            && self.use_python_app_entrypoint == other.use_python_app_entrypoint
            && self.version_independent.is_some() == other.version_independent.is_some()
            && self.site_packages_path == other.site_packages_path
    }
}

impl Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Build {{ number: {}, string: {}, script: {}, noarch: {}, skip: [{}] }}",
            self.number
                .as_ref()
                .map(|v| format!("{}", v))
                .unwrap_or_else(|| "inherited".to_string()),
            self.string.as_ref().into_iter().format(", "),
            self.script,
            self.noarch
                .as_ref()
                .map(|v| format!("{:?}", v))
                .unwrap_or_default(),
            self.skip.iter().format(", "),
        )
    }
}

impl Build {
    /// Collect all variables used in the build section
    pub fn used_variables(&self) -> Vec<String> {
        let Build {
            number,
            string,
            script,
            noarch,
            python,
            skip,
            always_copy_files,
            always_include_files,
            merge_build_and_host_envs,
            files,
            dynamic_linking,
            variant,
            prefix_detection,
            post_process,
            signing,
        } = self;

        let mut vars = Vec::new();

        if let Some(number) = number {
            vars.extend(number.used_variables());
        }

        if let Some(string) = string {
            vars.extend(string.used_variables());
        }

        vars.extend(script.used_variables());

        if let Some(noarch) = noarch {
            vars.extend(noarch.used_variables());
        }

        // Skip values are Jinja boolean expressions (not templates with ${{ }})
        // We need to parse them as expressions to extract variable names
        for item in skip {
            // First get any variables from conditionals (if/then/else)
            vars.extend(item.used_variables());

            // For concrete string values, parse as Jinja expression to extract variables
            if let Some(value) = item.as_value()
                && let Some(expr_str) = value.as_concrete()
            {
                // Try to parse as JinjaExpression to extract variables
                if let Ok(expr) = rattler_build_jinja::JinjaExpression::new(expr_str.clone()) {
                    vars.extend(expr.used_variables().iter().cloned());
                }
            }
        }

        let PythonBuild {
            entry_points,
            skip_pyc_compilation,
            use_python_app_entrypoint,
            version_independent,
            site_packages_path,
        } = python;

        vars.extend(entry_points.used_variables());
        vars.extend(skip_pyc_compilation.used_variables());
        vars.extend(use_python_app_entrypoint.used_variables());

        if let Some(version_independent) = version_independent {
            vars.extend(version_independent.used_variables());
        }

        if let Some(site_packages_path) = site_packages_path {
            vars.extend(site_packages_path.used_variables());
        }

        vars.extend(always_copy_files.used_variables());
        vars.extend(always_include_files.used_variables());
        vars.extend(merge_build_and_host_envs.used_variables());
        vars.extend(files.used_variables());

        // Dynamic linking
        let DynamicLinking {
            rpaths,
            binary_relocation,
            missing_dso_allowlist,
            rpath_allowlist,
            overdepending_behavior,
            overlinking_behavior,
        } = dynamic_linking;

        vars.extend(rpaths.used_variables());

        match binary_relocation {
            BinaryRelocation::Boolean(val) => {
                vars.extend(val.used_variables());
            }
            BinaryRelocation::Patterns(list) => {
                for item in list {
                    vars.extend(item.used_variables());
                }
            }
        }

        vars.extend(missing_dso_allowlist.used_variables());
        vars.extend(rpath_allowlist.used_variables());

        if let Some(overdepending_behavior) = overdepending_behavior {
            vars.extend(overdepending_behavior.used_variables());
        }

        if let Some(overlinking_behavior) = overlinking_behavior {
            vars.extend(overlinking_behavior.used_variables());
        }

        // Variant
        let VariantKeyUsage {
            use_keys,
            ignore_keys,
            down_prioritize_variant,
        } = variant;

        vars.extend(use_keys.used_variables());
        vars.extend(ignore_keys.used_variables());

        if let Some(down_prioritize) = down_prioritize_variant {
            vars.extend(down_prioritize.used_variables());
        }

        // Prefix detection
        let PrefixDetection {
            force_file_type,
            ignore,
            ignore_binary_files,
        } = prefix_detection;

        let ForceFileType { text, binary } = force_file_type;

        vars.extend(text.used_variables());
        vars.extend(binary.used_variables());

        match ignore {
            PrefixIgnore::Boolean(val) => {
                vars.extend(val.used_variables());
            }
            PrefixIgnore::Patterns(list) => {
                vars.extend(list.used_variables());
            }
        }
        vars.extend(ignore_binary_files.used_variables());

        // Post-process (handle conditional items)
        vars.extend(post_process.used_variables());
        collect_post_process_vars(post_process.iter(), &mut vars);

        // Signing
        if let Some(macos) = &signing.macos {
            vars.extend(macos.identity.used_variables());
            if let Some(keychain) = &macos.keychain {
                vars.extend(keychain.used_variables());
            }
            if let Some(entitlements) = &macos.entitlements {
                vars.extend(entitlements.used_variables());
            }
            vars.extend(macos.options.used_variables());
        }
        if let Some(windows) = &signing.windows {
            if let Some(signtool) = &windows.signtool {
                vars.extend(signtool.certificate_file.used_variables());
                if let Some(password) = &signtool.certificate_password_env {
                    vars.extend(password.used_variables());
                }
            }
            if let Some(azure) = &windows.azure_trusted_signing {
                vars.extend(azure.endpoint.used_variables());
                vars.extend(azure.account_name.used_variables());
                vars.extend(azure.certificate_profile.used_variables());
            }
            if let Some(timestamp_url) = &windows.timestamp_url {
                vars.extend(timestamp_url.used_variables());
            }
            if let Some(digest_algorithm) = &windows.digest_algorithm {
                vars.extend(digest_algorithm.used_variables());
            }
        }

        vars.sort();
        vars.dedup();
        vars
    }
}

/// Helper function to recursively collect variables from PostProcess items
/// This handles both concrete values and nested conditionals
fn collect_post_process_vars<'a>(
    items: impl Iterator<Item = &'a Item<PostProcess>>,
    vars: &mut Vec<String>,
) {
    for item in items {
        match item {
            Item::Value(value) => {
                // For concrete values, extract variables from fields
                if let Some(pp) = value.as_concrete() {
                    vars.extend(pp.files.used_variables());
                    vars.extend(pp.regex.used_variables());
                    vars.extend(pp.replacement.used_variables());
                }
            }
            Item::Conditional(cond) => {
                // Recursively collect from then branch
                collect_post_process_vars(cond.then.iter(), vars);
                // Recursively collect from else branch if present
                if let Some(else_value) = &cond.else_value {
                    collect_post_process_vars(else_value.iter(), vars);
                }
            }
        }
    }
}
