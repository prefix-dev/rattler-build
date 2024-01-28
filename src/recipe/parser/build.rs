use std::collections::HashSet;
use std::str::FromStr;

use globset::GlobSet;
use rattler_conda_types::{package::EntryPoint, NoArchType};
use serde::{Deserialize, Serialize};

use super::glob_vec::GlobVec;
use super::{Dependency, FlattenErrors};
use crate::recipe::custom_yaml::RenderedSequenceNode;
use crate::recipe::parser::script::Script;
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
    pub use_keys: HashSet<String>,
    /// The keys to ignore
    pub ignore_keys: HashSet<String>,
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
        let mut variantkeyusage = VariantKeyUsage::default();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "use_keys" => {
                    let vec: Vec<String> = value.try_convert(key_str)?;
                    variantkeyusage.use_keys = HashSet::from_iter(vec);
                }
                "ignore_keys" => {
                    let vec: Vec<String> = value.try_convert(key_str)?;
                    variantkeyusage.ignore_keys = HashSet::from_iter(vec);
                }
                invalid => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                    )]);
                }
            }
        }

        Ok(variantkeyusage)
    }
}

impl VariantKeyUsage {
    fn is_default(&self) -> bool {
        let VariantKeyUsage {
            use_keys,
            ignore_keys,
        } = self;
        use_keys.is_empty() && ignore_keys.is_empty()
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) skip: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) dynamic_linking: Option<DynamicLinking>,
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
        self.skip
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
    pub const fn dynamic_linking(&self) -> Option<&DynamicLinking> {
        self.dynamic_linking.as_ref()
    }

    /// Check if the build should be skipped.
    pub fn is_skip_build(&self) -> bool {
        self.skip()
    }

    pub fn always_copy_files(&self) -> Option<&GlobSet> {
        self.always_copy_files.globset()
    }

    pub fn always_include_files(&self) -> Option<&GlobSet> {
        self.always_include_files.globset()
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
            variant
        }

        Ok(build)
    }
}

/// Settings for shared libraries and executables.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DynamicLinking {
    /// List of rpaths to use (linux only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) rpaths: Vec<String>,
    /// Whether to relocate binaries or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) binary_relocation: Option<BinaryRelocation>,
    /// Allow linking against libraries that are not in the run requirements
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) missing_dso_allowlist: GlobVec,
    /// Allow runpath / rpath to point to these locations outside of the environment.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub(super) rpath_allowlist: GlobVec,
    /// What to do when detecting overdepending.
    #[serde(default)]
    pub(super) overdepending_behavior: LinkingCheckBehavior,
    /// What to do when detecting overlinking.
    #[serde(default)]
    pub(super) overlinking_behavior: LinkingCheckBehavior,
}

impl DynamicLinking {
    /// Get the rpaths.
    pub fn rpaths(&self) -> Vec<String> {
        if self.rpaths.is_empty() {
            vec![String::from("lib/")]
        } else {
            self.rpaths.clone()
        }
    }

    // Get the binary relocation settings.
    pub fn binary_relocation(&self) -> Option<BinaryRelocation> {
        self.binary_relocation.clone()
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

/// Settings for relocating binaries.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum BinaryRelocation {
    /// Relocate all binaries.
    All(bool),
    /// Relocate specific paths.
    SpecificPaths(GlobVec),
}

impl Default for BinaryRelocation {
    fn default() -> Self {
        Self::All(true)
    }
}

impl BinaryRelocation {
    /// Return the paths to relocate.
    pub fn relocate_paths(&self) -> Option<&GlobSet> {
        match self {
            BinaryRelocation::All(_) => None,
            BinaryRelocation::SpecificPaths(paths) => paths.globset(),
        }
    }

    /// Returns true if there will be no relocation.
    pub fn no_relocation(&self) -> bool {
        self == &Self::All(false)
    }
}

impl TryConvertNode<BinaryRelocation> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<BinaryRelocation, Vec<PartialParsingError>> {
        if let Some(sequence) = self.as_sequence() {
            sequence.try_convert(name)
        } else if let Some(scalar) = self.as_scalar() {
            scalar.try_convert(name)
        } else {
            Err(vec![
                _partialerror!(*self.span(), ErrorKind::ExpectedScalar),
                _partialerror!(*self.span(), ErrorKind::ExpectedSequence),
            ])
        }
    }
}

impl TryConvertNode<BinaryRelocation> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<BinaryRelocation, Vec<PartialParsingError>> {
        let globvec: GlobVec = self.try_convert(name)?;
        Ok(BinaryRelocation::SpecificPaths(globvec))
    }
}

impl TryConvertNode<BinaryRelocation> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<BinaryRelocation, Vec<PartialParsingError>> {
        let mut binary_relocation = BinaryRelocation::default();
        if let Some(relocate) = self.as_bool() {
            binary_relocation = BinaryRelocation::All(relocate);
        }
        Ok(binary_relocation)
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
        validate_keys!(python, self.iter(), entry_points, skip_pyc_compilation);
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
            )]
        })
    }
}
