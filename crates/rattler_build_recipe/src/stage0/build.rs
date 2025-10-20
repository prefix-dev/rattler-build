use std::fmt::Display;

use itertools::Itertools as _;
use rattler_conda_types::package::EntryPoint;
use serde::{Deserialize, Serialize};

use crate::stage0::types::{ConditionalList, IncludeExclude, ScriptContent, Value};

/// Default build number is 0
fn default_build_number() -> Value<u64> {
    Value::new_concrete(0, None)
}

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
    #[serde(default = "default_build_number")]
    pub number: Value<u64>,

    /// Build string (usually auto-generated from variant hash)
    pub string: Option<Value<String>>,

    /// Build script - either inline commands or a file path
    /// Default is `build.sh` on Unix, `build.bat` on Windows
    #[serde(default)]
    pub script: ConditionalList<ScriptContent>,

    /// Noarch type - "python" or "generic"
    pub noarch: Option<Value<String>>,

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
    pub merge_build_and_host_envs: bool,

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
    pub post_process: Vec<PostProcess>,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            number: default_build_number(),
            string: None,
            script: ConditionalList::default(),
            noarch: None,
            python: PythonBuild::default(),
            skip: ConditionalList::default(),
            always_copy_files: ConditionalList::default(),
            always_include_files: ConditionalList::default(),
            merge_build_and_host_envs: false,
            files: IncludeExclude::default(),
            dynamic_linking: DynamicLinking::default(),
            variant: VariantKeyUsage::default(),
            prefix_detection: PrefixDetection::default(),
            post_process: Vec::new(),
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
    pub ignore_binary_files: bool,
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
    pub use_python_app_entrypoint: bool,

    /// Whether the package is Python version independent
    /// This is used for abi3 packages that are not tied to a specific Python version
    #[serde(default)]
    pub version_independent: bool,

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
            && self.version_independent == other.version_independent
            && self.site_packages_path == other.site_packages_path
    }
}

impl Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Build {{ number: {}, string: {}, script: [{}], noarch: {}, skip: [{}] }}",
            self.number,
            self.string.as_ref().into_iter().format(", "),
            self.script.iter().format(", "),
            self.noarch.as_ref().into_iter().format(", "),
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
            merge_build_and_host_envs: _,
            files,
            dynamic_linking,
            variant,
            prefix_detection,
            post_process,
        } = self;

        let mut vars = Vec::new();

        vars.extend(number.used_variables());

        if let Some(string) = string {
            vars.extend(string.used_variables());
        }

        for item in script {
            vars.extend(item.used_variables());
        }

        if let Some(noarch) = noarch {
            vars.extend(noarch.used_variables());
        }

        for item in skip {
            vars.extend(item.used_variables());
        }

        let PythonBuild {
            entry_points,
            skip_pyc_compilation,
            use_python_app_entrypoint: _,
            version_independent: _,
            site_packages_path,
        } = python;

        for item in entry_points {
            vars.extend(item.used_variables());
        }

        for item in skip_pyc_compilation {
            vars.extend(item.used_variables());
        }

        if let Some(site_packages_path) = site_packages_path {
            vars.extend(site_packages_path.used_variables());
        }

        for item in always_copy_files {
            vars.extend(item.used_variables());
        }

        for item in always_include_files {
            vars.extend(item.used_variables());
        }

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

        for item in rpaths {
            vars.extend(item.used_variables());
        }

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

        for item in missing_dso_allowlist {
            vars.extend(item.used_variables());
        }

        for item in rpath_allowlist {
            vars.extend(item.used_variables());
        }

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

        for item in use_keys {
            vars.extend(item.used_variables());
        }

        for item in ignore_keys {
            vars.extend(item.used_variables());
        }

        if let Some(down_prioritize) = down_prioritize_variant {
            vars.extend(down_prioritize.used_variables());
        }

        // Prefix detection
        let PrefixDetection {
            force_file_type,
            ignore,
            ignore_binary_files: _,
        } = prefix_detection;

        let ForceFileType { text, binary } = force_file_type;

        for item in text {
            vars.extend(item.used_variables());
        }

        for item in binary {
            vars.extend(item.used_variables());
        }

        match ignore {
            PrefixIgnore::Boolean(val) => {
                vars.extend(val.used_variables());
            }
            PrefixIgnore::Patterns(list) => {
                for item in list {
                    vars.extend(item.used_variables());
                }
            }
        }

        // Post-process
        for pp in post_process {
            let PostProcess {
                files,
                regex,
                replacement,
            } = pp;

            for item in files {
                vars.extend(item.used_variables());
            }
            vars.extend(regex.used_variables());
            vars.extend(replacement.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }
}
