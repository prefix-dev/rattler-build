//! Stage 1 Build - evaluated build configuration with concrete values
use rattler_build_jinja::{Jinja, Variable};
use rattler_build_script::Script;
use rattler_build_yaml_parser::ParseError;
use rattler_conda_types::{NoArchType, package::EntryPoint};
use serde::{Deserialize, Serialize};

use crate::stage1::HashInfo;

use super::{AllOrGlobVec, GlobVec};

/// RPaths configuration with a default value of ["lib/"] when empty
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rpaths(Vec<String>);

impl Rpaths {
    /// Create a new Rpaths from a vector
    pub fn new(paths: Vec<String>) -> Self {
        Self(paths)
    }

    /// Get the rpaths as a Vec, with default ["lib/"] if empty
    pub fn to_vec(&self) -> Vec<String> {
        if self.0.is_empty() {
            vec![String::from("lib/")]
        } else {
            self.0.clone()
        }
    }

    /// Check if the rpaths are empty (before applying defaults)
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get the inner vector (without applying defaults)
    pub fn inner(&self) -> &Vec<String> {
        &self.0
    }
}

/// Represents the state of the build string during evaluation
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BuildString {
    #[default]
    /// The default build string will be resolved as {prefix}h{hash}, e.g. `py312habc123f`
    Default,

    /// Unresolved template that needs hash substitution
    /// This state exists during evaluation before the hash is computed
    #[serde(skip)]
    Unresolved(String, Option<crate::Span>),

    /// Fully resolved build string with hash computed and substituted
    Resolved(String),
}

impl BuildString {
    /// Create an unresolved build string from a template
    pub fn unresolved(template: String, span: Option<crate::Span>) -> Self {
        BuildString::Unresolved(template, span)
    }

    /// Create a resolved build string
    pub fn resolved(value: String) -> Self {
        BuildString::Resolved(value)
    }

    /// Check if the build string is resolved
    pub fn is_resolved(&self) -> bool {
        matches!(self, BuildString::Resolved(_))
    }

    /// Get the resolved string value, if available
    pub fn as_resolved(&self) -> Option<&str> {
        match self {
            BuildString::Resolved(s) => Some(s),
            BuildString::Unresolved(_, _) | BuildString::Default => None,
        }
    }

    /// Get the string value, only if resolved
    /// Returns None for Default or Unresolved variants since they're not valid build strings yet
    pub fn as_str(&self) -> Option<&str> {
        match self {
            BuildString::Resolved(s) => Some(s),
            BuildString::Unresolved(_, _) | BuildString::Default => None,
        }
    }

    /// Resolve the build string by rendering the template with the hash value and build number
    pub fn resolve(
        &mut self,
        hash_info: &HashInfo,
        build_number: u64,
        context: &super::EvaluationContext,
    ) -> Result<(), ParseError> {
        match self {
            BuildString::Default => {
                // Generate default build string: <prefix>h<hash>_<build_number>
                let rendered = format!("{}h{}_{}", hash_info.prefix, hash_info.hash, build_number);
                *self = BuildString::Resolved(rendered);
            }
            BuildString::Unresolved(template, span) => {
                // Create a Jinja instance
                let mut jinja = Jinja::new(context.jinja_config().clone());

                // Add all context variables
                jinja = jinja.with_context(context.variables());

                // Add the hash variable to the context
                jinja.context_mut().insert(
                    "hash".to_string(),
                    Variable::from(hash_info.hash.as_str()).into(),
                );

                // Add the build_number variable to the context
                jinja.context_mut().insert(
                    "build_number".to_string(),
                    Variable::from(build_number as i64).into(),
                );

                // Render the template
                let rendered = jinja
                    .render_str(template)
                    .map_err(|e| ParseError::JinjaError {
                        message: format!("Failed to render build string template: {}", e)
                            .into_boxed_str(),
                        span: span.unwrap_or(crate::Span::new_blank()).into(),
                    })?;
                *self = BuildString::Resolved(rendered);
            }
            BuildString::Resolved(_) => {
                // Already resolved, nothing to do
            }
        }

        Ok(())
    }
}

impl From<String> for BuildString {
    fn from(s: String) -> Self {
        BuildString::Resolved(s)
    }
}

impl From<BuildString> for Option<String> {
    fn from(bs: BuildString) -> Self {
        match bs {
            BuildString::Resolved(s) => Some(s),
            BuildString::Unresolved(s, _) => Some(s),
            BuildString::Default => None,
        }
    }
}

impl AsRef<str> for BuildString {
    /// Get the build string as a string reference
    ///
    /// # Panics
    /// Panics if the build string is not yet resolved (Default or Unresolved state)
    fn as_ref(&self) -> &str {
        self.as_str()
            .expect("BuildString must be resolved before calling as_ref(). Call resolve() first.")
    }
}

/// Variant key usage configuration (evaluated)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VariantKeyUsage {
    /// Variant keys to use
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub use_keys: Vec<String>,
    /// Variant keys to ignore
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_keys: Vec<String>,
    /// Down-prioritize variant (negative priority value)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_prioritize_variant: Option<i32>,
}

impl VariantKeyUsage {
    /// Check if this is the default configuration
    pub fn is_default(&self) -> bool {
        self.use_keys.is_empty()
            && self.ignore_keys.is_empty()
            && self.down_prioritize_variant.is_none()
    }
}

/// Prefix detection configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrefixDetection {
    /// Force file type detection (text or binary)
    #[serde(default, skip_serializing_if = "ForceFileType::is_default")]
    pub force_file_type: ForceFileType,
    /// Files to ignore for prefix replacement
    #[serde(default, skip_serializing_if = "AllOrGlobVec::is_none")]
    pub ignore: AllOrGlobVec,
    /// Ignore binary files for prefix replacement (Unix only)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
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

impl PrefixDetection {
    /// Check if this is the default configuration
    pub fn is_default(&self) -> bool {
        self.force_file_type.is_default() && self.ignore.is_none() && !self.ignore_binary_files
    }
}

/// Force file type for prefix detection
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ForceFileType {
    /// Force these files to be treated as text files
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub text: GlobVec,
    /// Force these files to be treated as binary files
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub binary: GlobVec,
}

impl ForceFileType {
    /// Check if this is the default configuration
    pub fn is_default(&self) -> bool {
        self.text.is_empty() && self.binary.is_empty()
    }
}

/// Post-processing operations using regex replacements
#[derive(Debug, Clone)]
pub struct PostProcess {
    /// Files to apply this post-processing to
    pub files: GlobVec,
    /// Regular expression pattern to match
    pub regex: regex::Regex,
    /// Replacement string
    pub replacement: String,
}

impl PartialEq for PostProcess {
    fn eq(&self, other: &Self) -> bool {
        // Compare regex patterns as strings since Regex doesn't implement PartialEq
        self.files == other.files
            && self.regex.as_str() == other.regex.as_str()
            && self.replacement == other.replacement
    }
}

impl serde::Serialize for PostProcess {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("PostProcess", 3)?;
        state.serialize_field("files", &self.files)?;
        state.serialize_field("regex", self.regex.as_str())?;
        state.serialize_field("replacement", &self.replacement)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for PostProcess {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PostProcessHelper {
            files: GlobVec,
            regex: String,
            replacement: String,
        }

        let helper = PostProcessHelper::deserialize(deserializer)?;
        let regex = regex::Regex::new(&helper.regex).map_err(serde::de::Error::custom)?;

        Ok(PostProcess {
            files: helper.files,
            regex,
            replacement: helper.replacement,
        })
    }
}

/// Evaluated build configuration with all templates and conditionals resolved
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Build {
    /// Build number (increments with each rebuild)
    #[serde(default)]
    pub number: u64,

    /// Build string - can be unresolved (template) or resolved (with hash)
    /// Serializes only the resolved string value
    #[serde(default)]
    pub string: BuildString,

    /// Build script - contains script content, interpreter, environment variables, etc.
    #[serde(default, skip_serializing_if = "Script::is_default")]
    pub script: Script,

    /// Noarch type - "python" or "generic" if set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noarch: Option<NoArchType>,

    /// Python-specific configuration
    #[serde(default, skip_serializing_if = "PythonBuild::is_default")]
    pub python: PythonBuild,

    /// Skip conditions - can be boolean expressions or platform names
    /// For example: ["win", "platform == 'osx-64'"]
    /// Note: This field is not serialized to JSON output (like main branch)
    #[serde(default, skip)]
    pub skip: Vec<String>,

    /// Always copy these files (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub always_copy_files: GlobVec,

    /// Always include these files (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub always_include_files: GlobVec,

    /// Merge build and host environments
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge_build_and_host_envs: bool,

    /// Files to include in the package (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub files: GlobVec,

    /// Dynamic linking configuration
    #[serde(default, skip_serializing_if = "DynamicLinking::is_default")]
    pub dynamic_linking: DynamicLinking,

    /// Variant key usage configuration
    #[serde(default, skip_serializing_if = "VariantKeyUsage::is_default")]
    pub variant: VariantKeyUsage,

    /// Prefix detection configuration
    #[serde(default, skip_serializing_if = "PrefixDetection::is_default")]
    pub prefix_detection: PrefixDetection,

    /// Post-processing operations
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_process: Vec<PostProcess>,
}

/// Dynamic linking configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicLinking {
    /// RPaths to use (Linux/macOS only)
    /// Defaults to ["lib/"] when empty
    #[serde(default, skip_serializing_if = "Rpaths::is_empty")]
    pub rpaths: Rpaths,

    /// Binary relocation setting
    /// - All(true): relocate all binaries (default)
    /// - All(false): don't relocate any binaries
    /// - SpecificPaths(globs): relocate only specific paths
    #[serde(default, skip_serializing_if = "AllOrGlobVec::is_all")]
    pub binary_relocation: AllOrGlobVec,

    /// Allow these missing DSOs (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub missing_dso_allowlist: GlobVec,

    /// Allow rpath to point to these locations (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub rpath_allowlist: GlobVec,

    /// What to do when detecting overdepending (ignore or error)
    #[serde(default, skip_serializing_if = "LinkingCheckBehavior::is_ignore")]
    pub overdepending_behavior: LinkingCheckBehavior,

    /// What to do when detecting overlinking (ignore or error)
    #[serde(default, skip_serializing_if = "LinkingCheckBehavior::is_ignore")]
    pub overlinking_behavior: LinkingCheckBehavior,
}

impl Default for DynamicLinking {
    fn default() -> Self {
        Self {
            rpaths: Rpaths::default(),
            binary_relocation: AllOrGlobVec::All(true),
            missing_dso_allowlist: GlobVec::default(),
            rpath_allowlist: GlobVec::default(),
            overdepending_behavior: LinkingCheckBehavior::default(),
            overlinking_behavior: LinkingCheckBehavior::default(),
        }
    }
}

impl DynamicLinking {
    /// Check if this is the default configuration
    pub fn is_default(&self) -> bool {
        self.rpaths.is_empty()
            && self.binary_relocation.is_all()
            && self.missing_dso_allowlist.is_empty()
            && self.rpath_allowlist.is_empty()
            && self.overdepending_behavior.is_ignore()
            && self.overlinking_behavior.is_ignore()
    }
}

/// What to do during linking checks
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkingCheckBehavior {
    /// Ignore the issue (default)
    #[default]
    Ignore,
    /// Report the issue as an error
    Error,
}

impl LinkingCheckBehavior {
    /// Check if this is Ignore (the default)
    pub fn is_ignore(&self) -> bool {
        matches!(self, LinkingCheckBehavior::Ignore)
    }
}

/// Python-specific build configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonBuild {
    /// Python entry points (executable_name = module:function)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<EntryPoint>,

    /// Skip pyc compilation for these files (validated glob patterns)
    /// Only relevant for non-noarch Python packages
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub skip_pyc_compilation: GlobVec,

    /// Use Python.app on macOS for GUI applications
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_python_app_entrypoint: bool,

    /// Whether the package is Python version independent
    /// This is used for abi3 packages that are not tied to a specific Python version
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub version_independent: bool,

    /// The relative site-packages path that a Python build exports for other packages to use
    /// This setting only makes sense for the `python` package itself
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_packages_path: Option<String>,
}

impl PythonBuild {
    /// Check if this is the default configuration
    pub fn is_default(&self) -> bool {
        self.entry_points.is_empty()
            && self.skip_pyc_compilation.is_empty()
            && !self.use_python_app_entrypoint
            && !self.version_independent
            && self.site_packages_path.is_none()
    }
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
            && matches!(self.string, BuildString::Default)
            && self.script.is_default()
            && self.noarch.is_none()
            && self.python.entry_points.is_empty()
            && self.python.skip_pyc_compilation.is_empty()
            && !self.python.use_python_app_entrypoint
            && !self.python.version_independent
            && self.python.site_packages_path.is_none()
            && self.skip.is_empty()
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
        use rattler_build_script::ScriptContent;

        let build = Build {
            script: Script {
                content: ScriptContent::Commands(vec![
                    "echo hello".to_string(),
                    "make install".to_string(),
                ]),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(!build.is_default());
        assert!(!build.script.is_default());
    }
}
