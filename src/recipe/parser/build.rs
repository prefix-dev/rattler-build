use std::str::FromStr;

use globset::GlobSet;
use rattler_conda_types::{package::EntryPoint, NoArchType};
use serde::{Deserialize, Serialize};

use super::glob_vec::{AllOrGlobVec, GlobVec};
use super::{Dependency, FlattenErrors, SerializableRegex};
use crate::recipe::custom_yaml::RenderedSequenceNode;
use crate::recipe::parser::script::Script;
use crate::recipe::parser::skip::Skip;

use crate::validate_keys;
use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
};

/// The config for using or ignoring variant keys
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct VariantKeyUsage {
    /// The keys to use
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) use_keys: Vec<String>,
    /// The keys to ignore
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ignore_keys: Vec<String>,
    /// Down-prioritize variant by setting the priority to a negative value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) down_prioritize_variant: Option<i32>,
}

impl TryConvertNode<VariantKeyUsage> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<VariantKeyUsage, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<VariantKeyUsage> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<VariantKeyUsage, Vec<PartialParsingError>> {
        let mut variant = VariantKeyUsage::default();
        validate_keys!(
            variant,
            self.iter(),
            use_keys,
            ignore_keys,
            down_prioritize_variant
        );
        Ok(variant)
    }
}

impl VariantKeyUsage {
    fn is_default(&self) -> bool {
        self.use_keys.is_empty()
            && self.ignore_keys.is_empty()
            && self.down_prioritize_variant.is_none()
    }
}

/// The build options contain information about how to build the package and some additional
/// metadata about the package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Build {
    /// The build number is a number that should be incremented every time the recipe is built.
    pub(super) number: u64,
    /// The build string is usually set automatically as the hash of the variant configuration.
    /// It's possible to override this by setting it manually, but not recommended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) string: Option<String>,
    /// List of conditions under which to skip the build of the package.
    #[serde(default, skip)]
    pub(super) skip: Skip,
    /// The build script can be either a list of commands or a path to a script. By
    /// default, the build script is set to `build.sh` or `build.bat` on Unix and Windows respectively.
    #[serde(default, skip_serializing_if = "Script::is_default")]
    pub(super) script: Script,
    /// A noarch package runs on any platform. It can be either a python package or a generic package.
    #[serde(default, skip_serializing_if = "NoArchType::is_none")]
    pub(super) noarch: NoArchType,
    /// Python specific build configuration
    #[serde(default, skip_serializing_if = "Python::is_default")]
    pub(super) python: Python,
    /// Settings for shared libraries and executables
    #[serde(default, skip_serializing_if = "DynamicLinking::is_default")]
    pub(super) dynamic_linking: DynamicLinking,
    /// Setting to control wether to always copy a file
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) always_copy_files: GlobVec,
    /// Setting to control wether to always include a file (even if it is already present in the host env)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) always_include_files: GlobVec,
    /// Merge the build and host envs
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) merge_build_and_host_envs: bool,
    /// Variant ignore and use keys
    #[serde(default, skip_serializing_if = "VariantKeyUsage::is_default")]
    pub(super) variant: VariantKeyUsage,
    /// Prefix detection settings
    #[serde(default, skip_serializing_if = "PrefixDetection::is_default")]
    pub(super) prefix_detection: PrefixDetection,
    /// Post-process operations for regex based replacements
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) post_process: Vec<PostProcess>,
    /// Include files in the package
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) include_files: GlobVec,
}

/// Post process operations for regex based replacements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostProcess {
    pub files: GlobVec,
    pub regex: SerializableRegex,
    pub replacement: String,
}

impl Build {
    /// Get the merge build host flag.
    pub const fn merge_build_and_host_envs(&self) -> bool {
        self.merge_build_and_host_envs
    }

    /// Variant ignore and use keys
    pub(crate) const fn variant(&self) -> &VariantKeyUsage {
        &self.variant
    }

    /// Get the build number.
    pub const fn number(&self) -> u64 {
        self.number
    }

    /// Get the build string.
    pub fn string(&self) -> Option<&str> {
        self.string.as_deref()
    }

    /// Get the skip conditions.
    pub fn skip(&self) -> bool {
        self.skip.eval()
    }

    /// Get the build script.
    pub fn script(&self) -> &Script {
        &self.script
    }

    /// Get the noarch type.
    pub const fn noarch(&self) -> &NoArchType {
        &self.noarch
    }

    /// Python specific build configuration.
    pub const fn python(&self) -> &Python {
        &self.python
    }

    /// Settings for shared libraries and executables
    pub const fn dynamic_linking(&self) -> &DynamicLinking {
        &self.dynamic_linking
    }

    /// Get the always copy files settings.
    pub fn always_copy_files(&self) -> Option<&GlobSet> {
        self.always_copy_files.globset()
    }

    /// Get the always include files settings.
    pub fn always_include_files(&self) -> Option<&GlobSet> {
        self.always_include_files.globset()
    }

    /// Get the include files settings.
    pub fn include_files(&self) -> &GlobVec {
        &self.include_files
    }

    /// Get the prefix detection settings.
    pub const fn prefix_detection(&self) -> &PrefixDetection {
        &self.prefix_detection
    }

    /// Post-process operations for regex based replacements
    pub const fn post_process(&self) -> &Vec<PostProcess> {
        &self.post_process
    }
}

impl TryConvertNode<Build> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Build, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Build> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<Build, Vec<PartialParsingError>> {
        let mut build = Build::default();

        validate_keys! {
            build,
            self.iter(),
            number,
            string,
            skip,
            script,
            noarch,
            python,
            dynamic_linking,
            always_copy_files,
            always_include_files,
            merge_build_and_host_envs,
            variant,
            prefix_detection,
            post_process,
            include_files
        }

        Ok(build)
    }
}

/// Settings for shared libraries and executables.
#[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct DynamicLinking {
    /// List of rpaths to use (linux only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) rpaths: Vec<String>,
    /// Whether to relocate binaries or not.
    #[serde(default, skip_serializing_if = "AllOrGlobVec::is_all")]
    pub(super) binary_relocation: AllOrGlobVec,
    /// Allow linking against libraries that are not in the run requirements
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) missing_dso_allowlist: GlobVec,
    /// Allow runpath / rpath to point to these locations outside of the environment.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) rpath_allowlist: GlobVec,
    /// What to do when detecting overdepending.
    #[serde(default, skip_serializing_if = "LinkingCheckBehavior::is_default")]
    pub(super) overdepending_behavior: LinkingCheckBehavior,
    /// What to do when detecting overlinking.
    #[serde(default, skip_serializing_if = "LinkingCheckBehavior::is_default")]
    pub(super) overlinking_behavior: LinkingCheckBehavior,
}

impl DynamicLinking {
    /// Returns true if this is the default dynamic linking configuration.
    pub fn is_default(&self) -> bool {
        self == &DynamicLinking::default()
    }

    /// Get the rpaths.
    pub fn rpaths(&self) -> Vec<String> {
        if self.rpaths.is_empty() {
            vec![String::from("lib/")]
        } else {
            self.rpaths.clone()
        }
    }

    /// Get the binary relocation settings.
    pub fn binary_relocation(&self) -> &AllOrGlobVec {
        &self.binary_relocation
    }

    /// Get the missing DSO allowlist.
    pub fn missing_dso_allowlist(&self) -> Option<&GlobSet> {
        self.missing_dso_allowlist.globset()
    }

    /// Get the rpath allow list.
    pub fn rpath_allowlist(&self) -> Option<&GlobSet> {
        self.rpath_allowlist.globset()
    }

    /// Get the overdepending behavior.
    pub fn error_on_overdepending(&self) -> bool {
        self.overdepending_behavior == LinkingCheckBehavior::Error
    }

    /// Get the overlinking behavior.
    pub fn error_on_overlinking(&self) -> bool {
        self.overlinking_behavior == LinkingCheckBehavior::Error
    }
}

/// What to do during linking checks.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkingCheckBehavior {
    #[default]
    Ignore,
    Error,
}

impl LinkingCheckBehavior {
    /// Returns true if this is the default linking check behavior.
    pub fn is_default(&self) -> bool {
        self == &LinkingCheckBehavior::default()
    }
}

impl TryConvertNode<LinkingCheckBehavior> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<LinkingCheckBehavior, Vec<PartialParsingError>> {
        self.as_scalar()
            .cloned()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedScalar)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<LinkingCheckBehavior> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<LinkingCheckBehavior, Vec<PartialParsingError>> {
        match self.as_str() {
            "ignore" => Ok(LinkingCheckBehavior::Ignore),
            "error" => Ok(LinkingCheckBehavior::Error),
            _ => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedScalar,
                help = format!("valid options for {name} are `ignore` or `error`")
            )]),
        }
    }
}

impl TryConvertNode<DynamicLinking> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<DynamicLinking, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<DynamicLinking> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<DynamicLinking, Vec<PartialParsingError>> {
        let mut dynamic_linking = DynamicLinking::default();

        validate_keys!(
            dynamic_linking,
            self.iter(),
            rpaths,
            binary_relocation,
            missing_dso_allowlist,
            rpath_allowlist,
            overdepending_behavior,
            overlinking_behavior
        );

        Ok(dynamic_linking)
    }
}

impl TryConvertNode<Vec<PostProcess>> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Vec<PostProcess>, Vec<PartialParsingError>> {
        self.as_sequence()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedSequence)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Vec<PostProcess>> for RenderedSequenceNode {
    fn try_convert(&self, _name: &str) -> Result<Vec<PostProcess>, Vec<PartialParsingError>> {
        let mut post_process = Vec::new();

        for (idx, node) in self.iter().enumerate() {
            let pp = node.try_convert(&format!("post_process[{}]", idx))?;
            post_process.push(pp);
        }

        Ok(post_process)
    }
}

impl TryConvertNode<PostProcess> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PostProcess, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<PostProcess> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PostProcess, Vec<PartialParsingError>> {
        let mut post_process = PostProcess {
            files: GlobVec::default(),
            regex: SerializableRegex::default(),
            replacement: String::new(),
        };

        validate_keys!(post_process, self.iter(), files, regex, replacement);

        Ok(post_process)
    }
}

/// Python specific build configuration
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Python {
    /// For a Python noarch package to have executables it is necessary to specify the python entry points.
    /// These contain the name of the executable and the module + function that should be executed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<EntryPoint>,

    /// Skip pyc compilation for these files.
    /// This is useful for files that are not meant to be imported.
    /// Only relevant for non-noarch Python packages.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub skip_pyc_compilation: GlobVec,

    /// Wether to use the "app" entry point for Python (which hooks into the macOS GUI)
    /// This is only relevant for macOS.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_python_app_entrypoint: bool,
}

impl Python {
    /// Returns true if this is the default python configuration.
    pub fn is_default(&self) -> bool {
        self.entry_points.is_empty() && self.skip_pyc_compilation.is_empty()
    }
}

impl TryConvertNode<Python> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Python, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Python> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<Python, Vec<PartialParsingError>> {
        let mut python = Python::default();
        validate_keys!(
            python,
            self.iter(),
            entry_points,
            skip_pyc_compilation,
            use_python_app_entrypoint
        );
        Ok(python)
    }
}

/// Run exports are applied to downstream packages that depend on this package.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RunExports {
    /// Noarch run exports are the only ones looked at when building noarch packages.
    pub noarch: Vec<Dependency>,
    /// Strong run exports apply from the build and host env to the run env.
    pub strong: Vec<Dependency>,
    /// Strong run constrains add run_constrains from the build and host env.
    pub strong_constraints: Vec<Dependency>,
    /// Weak run exports apply from the host env to the run env.
    pub weak: Vec<Dependency>,
    /// Weak run constrains add run_constrains from the host env.
    pub weak_constraints: Vec<Dependency>,
}

impl RunExports {
    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constraints.is_empty()
            && self.weak.is_empty()
            && self.weak_constraints.is_empty()
    }

    /// Get all run exports from all configurations
    pub fn all(&self) -> impl Iterator<Item = &Dependency> {
        self.noarch
            .iter()
            .chain(self.strong.iter())
            .chain(self.strong_constraints.iter())
            .chain(self.weak.iter())
            .chain(self.weak_constraints.iter())
    }
}

impl TryConvertNode<RunExports> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, Vec<PartialParsingError>> {
        let mut run_exports = RunExports::default();

        let dep = self.try_convert(name)?;
        run_exports.weak.push(dep);

        Ok(run_exports)
    }
}

impl TryConvertNode<NoArchType> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<NoArchType, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedScalar,)])?
            .try_convert(name)
    }
}

impl TryConvertNode<NoArchType> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<NoArchType, Vec<PartialParsingError>> {
        let noarch = self.as_str();
        let noarch = match noarch {
            "python" => NoArchType::python(),
            "generic" => NoArchType::generic(),
            invalid => {
                return Err(vec![_partialerror!(
                    *self.span(),
                    ErrorKind::InvalidValue((name.to_string(), invalid.to_owned().into())),
                    help = format!("expected `python` or `generic` for {name}"),
                )]);
            }
        };
        Ok(noarch)
    }
}

impl TryConvertNode<EntryPoint> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<EntryPoint, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedScalar)])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<EntryPoint> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<EntryPoint, Vec<PartialParsingError>> {
        EntryPoint::from_str(self.as_str()).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::EntryPointParsing(err),
                help = format!("expected a string in the format of `command = module:function`")
            )]
        })
    }
}

/// Options to control the prefix replacement behavior at installation time
#[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct ForceFileType {
    /// Force these files to be detected as text files (just replace the string without padding)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub text: GlobVec,
    /// Force these files to be detected as binary files for prefix replacement
    /// (pad strings with null bytes to the right to match the length of the original file)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub binary: GlobVec,
}

impl ForceFileType {
    /// Returns true if this is the default force file type configuration.
    pub fn is_default(&self) -> bool {
        self.text.is_empty() && self.binary.is_empty()
    }
}

///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixDetection {
    /// Options to force if a file is detected as text or binary
    #[serde(default, skip_serializing_if = "ForceFileType::is_default")]
    pub force_file_type: ForceFileType,

    /// Ignore these files for prefix replacement
    #[serde(default, skip_serializing_if = "AllOrGlobVec::is_none")]
    pub ignore: AllOrGlobVec,

    /// Ignore binary files for prefix replacement (ignored on Windows)
    /// This option defaults to false on Unix
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
    /// Returns true if this is the default prefix detection configuration.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl TryConvertNode<PrefixDetection> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PrefixDetection, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<PrefixDetection> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PrefixDetection, Vec<PartialParsingError>> {
        let mut prefix_detection = PrefixDetection::default();
        validate_keys!(
            prefix_detection,
            self.iter(),
            force_file_type,
            ignore,
            ignore_binary_files
        );
        Ok(prefix_detection)
    }
}

impl TryConvertNode<ForceFileType> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<ForceFileType, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<ForceFileType> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<ForceFileType, Vec<PartialParsingError>> {
        let mut force_file_type = ForceFileType::default();
        validate_keys!(force_file_type, self.iter(), text, binary);
        Ok(force_file_type)
    }
}
