//! Stage 1 Build - evaluated build configuration with concrete values
use std::path::PathBuf;

use indexmap::IndexMap;
use rattler_build_jinja::Variable;
use rattler_build_script::{Script, ScriptContent};
use rattler_build_yaml_parser::ParseError;
use rattler_conda_types::{Flag, NoArchType, package::EntryPoint};
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
                let mut jinja = context.to_jinja();

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

/// The evaluated `run` payload for a build step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepRun {
    /// Multiple commands, joined by the selected interpreter.
    Commands(Vec<String>),
    /// A single inline script body.
    Command(String),
}

impl Default for StepRun {
    fn default() -> Self {
        Self::Commands(Vec::new())
    }
}

impl From<StepRun> for ScriptContent {
    fn from(value: StepRun) -> Self {
        match value {
            StepRun::Commands(commands) => Self::Commands(commands),
            StepRun::Command(command) => Self::Command(command),
        }
    }
}

impl From<&ScriptContent> for StepRun {
    fn from(value: &ScriptContent) -> Self {
        match value {
            ScriptContent::Commands(commands) => Self::Commands(commands.clone()),
            ScriptContent::Command(command) | ScriptContent::CommandOrPath(command) => {
                Self::Command(command.clone())
            }
            ScriptContent::Path(path) => Self::Command(path.to_string_lossy().into_owned()),
            ScriptContent::Default => Self::Commands(Vec::new()),
        }
    }
}

/// A stage1 build step with evaluated metadata and script content.
///
/// This is deliberately separate from [`Script`]: rendered recipes use
/// `build.steps[].run`, while `build.script` uses the normal script syntax.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Script content to execute for this step.
    pub run: StepRun,
    /// Optional interpreter override for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<String>,
    /// Environment variables scoped to this step only.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env: IndexMap<String, String>,
    /// Optional working directory for this step, relative to the host prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

impl Step {
    /// Create a step from an evaluated script payload.
    pub fn new(script: Script) -> Self {
        Self {
            run: StepRun::from(&script.content),
            interpreter: script.interpreter,
            env: script.env,
            cwd: script.cwd,
        }
    }

    /// Convert this step into the script representation used by the executor.
    pub fn to_script(&self) -> Script {
        Script {
            interpreter: self.interpreter.clone(),
            env: self.env.clone(),
            secrets: Vec::new(),
            content: self.run.clone().into(),
            cwd: self.cwd.clone(),
            content_explicit: false,
        }
    }
}

/// The executable build plan: either a single legacy script, or explicit
/// ordered build steps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildPlan {
    /// Legacy `build.script` mode. `Script::default()` preserves default
    /// `build.sh` / `build.bat` discovery.
    Script(Script),
    /// Explicit `build.steps` mode. An empty vector is meaningful: it disables
    /// legacy default script discovery.
    Steps(Vec<Step>),
}

impl Default for BuildPlan {
    fn default() -> Self {
        Self::Script(Script::default())
    }
}

impl BuildPlan {
    /// Returns true if this is the default script-discovery plan.
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Script(script) if script.is_default())
    }

    /// Returns the script in legacy script mode.
    pub fn script(&self) -> Option<&Script> {
        match self {
            Self::Script(script) => Some(script),
            Self::Steps(_) => None,
        }
    }

    /// Returns the steps in explicit steps mode.
    pub fn steps(&self) -> Option<&[Step]> {
        match self {
            Self::Script(_) => None,
            Self::Steps(steps) => Some(steps.as_slice()),
        }
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
    /// Down-prioritize this variant. Higher values make the variant less
    /// preferred; the magnitude of the value is used (the sign is ignored).
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
        #[serde(deny_unknown_fields)]
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
#[serde(try_from = "BuildDeserialize")]
pub struct Build {
    /// Build number (increments with each rebuild)
    /// None means inherit from top-level, Some(n) means use n (even if n is 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,

    /// Build string - can be unresolved (template) or resolved (with hash)
    /// Serializes only the resolved string value
    #[serde(default)]
    pub string: BuildString,

    /// Executable build plan: either a single legacy script or explicit steps.
    #[serde(default, flatten, skip_serializing_if = "BuildPlan::is_default")]
    pub plan: BuildPlan,

    /// Noarch type - "python" or "generic" if set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noarch: Option<NoArchType>,

    /// V3 package variant flags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<Flag>,

    /// Python-specific configuration
    #[serde(default, skip_serializing_if = "PythonBuild::is_default")]
    pub python: PythonBuild,

    /// Whether the build should be skipped (pre-evaluated from skip conditions).
    /// Skip conditions are evaluated eagerly during recipe evaluation, before
    /// the variant gets noarch overrides
    #[serde(default, skip)]
    pub skip: bool,

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

#[derive(Default)]
enum PresentField<T> {
    #[default]
    Missing,
    Present(T),
}

impl<'de, T> Deserialize<'de> for PresentField<T>
where
    T: serde::de::DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = serde_yaml::Value::deserialize(deserializer)?;
        if value.is_null() {
            return Err(D::Error::custom("null is not a valid value for this field"));
        }
        T::deserialize(value)
            .map(Self::Present)
            .map_err(D::Error::custom)
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildDeserialize {
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    string: BuildString,
    #[serde(default)]
    script: PresentField<Script>,
    #[serde(default)]
    steps: PresentField<Vec<Step>>,
    #[serde(default)]
    noarch: Option<NoArchType>,
    #[serde(default)]
    flags: Vec<Flag>,
    #[serde(default)]
    python: PythonBuild,
    #[serde(default)]
    skip: bool,
    #[serde(default)]
    always_copy_files: GlobVec,
    #[serde(default)]
    always_include_files: GlobVec,
    #[serde(default)]
    merge_build_and_host_envs: bool,
    #[serde(default)]
    files: GlobVec,
    #[serde(default)]
    dynamic_linking: DynamicLinking,
    #[serde(default)]
    variant: VariantKeyUsage,
    #[serde(default)]
    prefix_detection: PrefixDetection,
    #[serde(default)]
    post_process: Vec<PostProcess>,
}

impl TryFrom<BuildDeserialize> for Build {
    type Error = String;

    fn try_from(raw: BuildDeserialize) -> Result<Self, Self::Error> {
        let plan = match (raw.script, raw.steps) {
            (PresentField::Present(_), PresentField::Present(_)) => {
                return Err(
                    "`script` and `steps` are mutually exclusive; use one or the other".to_string(),
                );
            }
            (PresentField::Missing, PresentField::Present(steps)) => BuildPlan::Steps(steps),
            (PresentField::Present(script), PresentField::Missing) => BuildPlan::Script(script),
            (PresentField::Missing, PresentField::Missing) => BuildPlan::Script(Script::default()),
        };

        Ok(Self {
            number: raw.number,
            string: raw.string,
            plan,
            noarch: raw.noarch,
            flags: raw.flags,
            python: raw.python,
            skip: raw.skip,
            always_copy_files: raw.always_copy_files,
            always_include_files: raw.always_include_files,
            merge_build_and_host_envs: raw.merge_build_and_host_envs,
            files: raw.files,
            dynamic_linking: raw.dynamic_linking,
            variant: raw.variant,
            prefix_detection: raw.prefix_detection,
            post_process: raw.post_process,
        })
    }
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
            number: Some(number),
            ..Default::default()
        }
    }

    /// Check if the build section is empty (all default values)
    pub fn is_default(&self) -> bool {
        self.number.is_none()
            && matches!(self.string, BuildString::Default)
            && self.plan.is_default()
            && self.noarch.is_none()
            && self.flags.is_empty()
            && self.python.entry_points.is_empty()
            && self.python.skip_pyc_compilation.is_empty()
            && !self.python.use_python_app_entrypoint
            && !self.python.version_independent
            && self.python.site_packages_path.is_none()
            && !self.skip
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
        assert_eq!(build.number, None);
    }

    #[test]
    fn test_build_with_number() {
        let build = Build::with_number(5);
        assert_eq!(build.number, Some(5));
        assert!(!build.is_default());
    }

    #[test]
    fn test_build_with_script() {
        use rattler_build_script::ScriptContent;

        let build = Build {
            plan: BuildPlan::Script(Script {
                content: ScriptContent::Commands(vec![
                    "echo hello".to_string(),
                    "make install".to_string(),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(!build.is_default());
        assert!(!build.plan.is_default());
    }

    #[test]
    fn test_build_with_steps_is_not_default_and_serializes_steps() {
        use rattler_build_script::ScriptContent;

        let build = Build {
            plan: BuildPlan::Steps(vec![Step::new(Script {
                interpreter: Some("bash".to_string()),
                env: [("FOO".to_string(), "bar".to_string())]
                    .into_iter()
                    .collect(),
                content: ScriptContent::Commands(vec!["echo step".to_string()]),
                cwd: Some("subdir".into()),
                ..Default::default()
            })]),
            ..Default::default()
        };

        assert!(!build.is_default());
        let yaml = serde_yaml::to_string(&build).unwrap();
        assert!(yaml.contains("steps:"), "{yaml}");
        assert!(yaml.contains("run:"), "{yaml}");
        assert!(yaml.contains("echo step"), "{yaml}");
        assert!(yaml.contains("interpreter: bash"), "{yaml}");
        assert!(yaml.contains("cwd: subdir"), "{yaml}");
        assert!(!yaml.contains("source_index"), "{yaml}");

        let recipe_yaml = format!(
            "package:\n  name: test-pkg\n  version: 1.0.0\nbuild:\n{}",
            yaml.lines()
                .map(|line| format!("  {line}\n"))
                .collect::<String>()
        );
        let parsed = crate::stage0::parse_recipe_from_source(&recipe_yaml).unwrap();
        assert_eq!(parsed.build.plan.steps().map(<[_]>::len), Some(1));

        let roundtripped: Build = serde_yaml::from_str(&yaml).unwrap();
        let steps = roundtripped.plan.steps().expect("steps mode");
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].run,
            StepRun::Commands(vec!["echo step".to_string()])
        );
    }

    #[test]
    fn test_build_deserialize_rejects_unknown_step_fields() {
        let yaml = r#"
steps:
  - if: win
    run: echo windows
"#;

        let result = serde_yaml::from_str::<Build>(yaml);
        assert!(
            result.is_err(),
            "expected unknown step fields to be rejected"
        );
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_build_deserialize_rejects_unknown_build_fields() {
        let result = serde_yaml::from_str::<Build>("unexpected: true\n");
        assert!(
            result.is_err(),
            "expected unknown build fields to be rejected"
        );
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_build_deserialize_rejects_unknown_post_process_fields() {
        let yaml = r#"
post_process:
  - files:
      - bin/*
    regex: foo
    replacement: bar
    unexpected: true
"#;

        let result = serde_yaml::from_str::<Build>(yaml);
        assert!(
            result.is_err(),
            "expected unknown post_process fields to be rejected"
        );
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_build_deserialize_rejects_script_and_steps() {
        let yaml = r#"
script: echo script
steps:
  - run: echo step
"#;

        let result = serde_yaml::from_str::<Build>(yaml);
        assert!(result.is_err(), "expected script+steps to be rejected");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("mutually exclusive")
        );
    }

    #[test]
    fn test_build_deserialize_rejects_null_script_or_steps() {
        for yaml in ["script:\n", "steps:\n", "script:\nsteps: []\n"] {
            let result = serde_yaml::from_str::<Build>(yaml);
            assert!(
                result.is_err(),
                "expected null script/steps to be rejected: {yaml:?}"
            );
        }
    }

    #[test]
    fn test_explicit_empty_steps_serializes_steps_mode() {
        let build = Build {
            plan: BuildPlan::Steps(Vec::new()),
            ..Default::default()
        };

        assert!(!build.is_default());
        let yaml = serde_yaml::to_string(&build).unwrap();
        assert!(yaml.contains("steps: []"), "{yaml}");

        let roundtripped: Build = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(roundtripped.plan.steps().map(<[_]>::len), Some(0));
    }
}
