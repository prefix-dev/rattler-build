//! Stage 1 Build - evaluated build configuration with concrete values
use super::{all_or_glob_vec::AllOrGlobVec, glob_vec::GlobVec};

/// Variant key usage configuration (evaluated)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VariantKeyUsage {
    /// Variant keys to use
    pub use_keys: Vec<String>,
    /// Variant keys to ignore
    pub ignore_keys: Vec<String>,
    /// Down-prioritize variant (negative priority value)
    pub down_prioritize_variant: Option<i32>,
}

/// Prefix detection configuration
#[derive(Debug, Clone, PartialEq)]
pub struct PrefixDetection {
    /// Force file type detection (text or binary)
    pub force_file_type: ForceFileType,
    /// Files to ignore for prefix replacement
    pub ignore: AllOrGlobVec,
    /// Ignore binary files for prefix replacement (Unix only)
    pub ignore_binary_files: bool,
}

impl Default for PrefixDetection {
    fn default() -> Self {
        Self {
            force_file_type: ForceFileType::default(),
            ignore: AllOrGlobVec::All(false),
            ignore_binary_files: false,
        }
    }
}

/// Force file type for prefix detection
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForceFileType {
    /// Force these files to be treated as text files
    pub text: GlobVec,
    /// Force these files to be treated as binary files
    pub binary: GlobVec,
}

/// Post-processing operations using regex replacements
#[derive(Debug, Clone, PartialEq)]
pub struct PostProcess {
    /// Files to apply this post-processing to
    pub files: GlobVec,
    /// Regular expression pattern to match
    pub regex: String,
    /// Replacement string
    pub replacement: String,
}

/// Evaluated build configuration with all templates and conditionals resolved
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Build {
    /// Build number (increments with each rebuild)
    pub number: u64,

    /// Build string (usually auto-generated from variant hash)
    pub string: Option<String>,

    /// Build script - list of commands or reference to a file
    pub script: Vec<String>,

    /// Noarch type - "python" or "generic" if set
    pub noarch: Option<NoArchType>,

    /// Python-specific configuration
    pub python: PythonBuild,

    /// Skip condition (evaluated boolean as string "true" or "false")
    pub skip: Option<String>,

    /// Always copy these files (validated glob patterns)
    pub always_copy_files: GlobVec,

    /// Always include these files (validated glob patterns)
    pub always_include_files: GlobVec,

    /// Merge build and host environments
    pub merge_build_and_host_envs: bool,

    /// Files to include in the package (validated glob patterns)
    pub files: GlobVec,

    /// Dynamic linking configuration
    pub dynamic_linking: DynamicLinking,

    /// Variant key usage configuration
    pub variant: VariantKeyUsage,

    /// Prefix detection configuration
    pub prefix_detection: PrefixDetection,

    /// Post-processing operations
    pub post_process: Vec<PostProcess>,
}

/// Dynamic linking configuration
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicLinking {
    /// RPaths to use (Linux/macOS only)
    pub rpaths: Vec<String>,

    /// Binary relocation setting
    /// - All(true): relocate all binaries (default)
    /// - All(false): don't relocate any binaries
    /// - SpecificPaths(globs): relocate only specific paths
    pub binary_relocation: AllOrGlobVec,

    /// Allow these missing DSOs (validated glob patterns)
    pub missing_dso_allowlist: GlobVec,

    /// Allow rpath to point to these locations (validated glob patterns)
    pub rpath_allowlist: GlobVec,

    /// What to do when detecting overdepending (ignore or error)
    pub overdepending_behavior: LinkingCheckBehavior,

    /// What to do when detecting overlinking (ignore or error)
    pub overlinking_behavior: LinkingCheckBehavior,
}

impl Default for DynamicLinking {
    fn default() -> Self {
        Self {
            rpaths: Vec::new(),
            binary_relocation: AllOrGlobVec::All(true),
            missing_dso_allowlist: GlobVec::default(),
            rpath_allowlist: GlobVec::default(),
            overdepending_behavior: LinkingCheckBehavior::default(),
            overlinking_behavior: LinkingCheckBehavior::default(),
        }
    }
}

/// What to do during linking checks
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LinkingCheckBehavior {
    /// Ignore the issue (default)
    #[default]
    Ignore,
    /// Report the issue as an error
    Error,
}

/// Python-specific build configuration
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PythonBuild {
    /// Python entry points (executable_name = module:function)
    pub entry_points: Vec<String>,

    /// Skip pyc compilation for these files (validated glob patterns)
    /// Only relevant for non-noarch Python packages
    pub skip_pyc_compilation: GlobVec,

    /// Use Python.app on macOS for GUI applications
    pub use_python_app_entrypoint: bool,

    /// Whether the package is Python version independent
    /// This is used for abi3 packages that are not tied to a specific Python version
    pub version_independent: bool,

    /// The relative site-packages path that a Python build exports for other packages to use
    /// This setting only makes sense for the `python` package itself
    pub site_packages_path: Option<String>,
}

/// NoArch type for platform-independent packages
#[derive(Debug, Clone, PartialEq)]
pub enum NoArchType {
    /// Python noarch package (pure Python, no compiled extensions)
    Python,
    /// Generic noarch package (platform-independent)
    Generic,
}

impl Build {
    /// Create a new build configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a build with a specific number
    pub fn with_number(number: u64) -> Self {
        Self {
            number,
            ..Default::default()
        }
    }

    /// Check if the build section is empty (all default values)
    pub fn is_default(&self) -> bool {
        self.number == 0
            && self.string.is_none()
            && self.script.is_empty()
            && self.noarch.is_none()
            && self.python.entry_points.is_empty()
            && self.python.skip_pyc_compilation.is_empty()
            && !self.python.use_python_app_entrypoint
            && !self.python.version_independent
            && self.python.site_packages_path.is_none()
            && self.skip.is_none()
            && self.always_copy_files.is_empty()
            && self.always_include_files.is_empty()
            && !self.merge_build_and_host_envs
            && self.files.is_empty()
            && self.dynamic_linking.rpaths.is_empty()
            && self.dynamic_linking.binary_relocation.is_all()
            && self.dynamic_linking.missing_dso_allowlist.is_empty()
            && self.dynamic_linking.rpath_allowlist.is_empty()
            && self.dynamic_linking.overdepending_behavior == LinkingCheckBehavior::Ignore
            && self.dynamic_linking.overlinking_behavior == LinkingCheckBehavior::Ignore
            && self.variant.use_keys.is_empty()
            && self.variant.ignore_keys.is_empty()
            && self.variant.down_prioritize_variant.is_none()
            && self.prefix_detection.force_file_type.text.is_empty()
            && self.prefix_detection.force_file_type.binary.is_empty()
            && self.prefix_detection.ignore.is_none()
            && !self.prefix_detection.ignore_binary_files
            && self.post_process.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_creation() {
        let build = Build::new();
        assert!(build.is_default());
        assert_eq!(build.number, 0);
    }

    #[test]
    fn test_build_with_number() {
        let build = Build::with_number(5);
        assert_eq!(build.number, 5);
        assert!(!build.is_default());
    }

    #[test]
    fn test_build_with_script() {
        let build = Build {
            script: vec!["echo hello".to_string(), "make install".to_string()],
            ..Default::default()
        };

        assert!(!build.is_default());
        assert_eq!(build.script.len(), 2);
    }

    #[test]
    fn test_noarch_python() {
        let build = Build {
            noarch: Some(NoArchType::Python),
            ..Default::default()
        };

        assert!(!build.is_default());
        assert!(matches!(build.noarch, Some(NoArchType::Python)));
    }
}
