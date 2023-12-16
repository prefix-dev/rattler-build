use std::str::FromStr;

use rattler_conda_types::{package::EntryPoint, NoArchType};
use serde::{Deserialize, Serialize};

use super::{Dependency, FlattenErrors};
use crate::recipe::parser::script::Script;
use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
};

/// A helper method to skip serializing the `skip` field if it's false.
fn should_not_serialize_skip(skip: &bool) -> bool {
    !skip
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
    #[serde(default, skip_serializing_if = "should_not_serialize_skip")]
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
    // TODO: Add and parse the rest of the fields
}

impl Build {
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

    /// Check if the build should be skipped.
    pub fn is_skip_build(&self) -> bool {
        self.skip()
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

        self.iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "number" => {
                        build.number = value.try_convert(key_str)?;
                    }
                    "string" => {
                        build.string = value.try_convert(key_str)?;
                    }
                    "skip" => {
                        let conds: Vec<bool> = value.try_convert(key_str)?;
                        build.skip = conds.iter().any(|&v| v);
                    }
                    "script" => build.script = value.try_convert(key_str)?,
                    "noarch" => {
                        build.noarch = value.try_convert(key_str)?;
                    }
                    "python" => {
                        build.python = value.try_convert(key_str)?;
                    }
                    invalid => {
                        return Err(vec![_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(invalid.to_string().into()),
                        )]);
                    }
                }
                Ok(())
            })
            .flatten_errors()?;

        Ok(build)
    }
}

/// Python specific build configuration
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Python {
    /// For a Python noarch package to have executables it is necessary to specify the python entry points.
    /// These contain the name of the executable and the module + function that should be executed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) entry_points: Vec<EntryPoint>,
}

impl Python {
    /// Get the entry points.
    pub fn entry_points(&self) -> &[EntryPoint] {
        self.entry_points.as_slice()
    }

    /// Returns true if this is the default python configuration.
    pub fn is_default(&self) -> bool {
        self.entry_points.is_empty()
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

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "entry_points" => {
                    python.entry_points = value.try_convert(key_str)?;
                }
                invalid => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                    )]);
                }
            }
        }

        Ok(python)
    }
}

/// Run exports are applied to downstream packages that depend on this package.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RunExports {
    /// Noarch run exports are the only ones looked at when building noarch packages.
    pub(super) noarch: Vec<Dependency>,
    /// Strong run exports apply from the build and host env to the run env.
    pub(super) strong: Vec<Dependency>,
    /// Strong run constrains add run_constrains from the build and host env.
    pub(super) strong_constrains: Vec<Dependency>,
    /// Weak run exports apply from the host env to the run env.
    pub(super) weak: Vec<Dependency>,
    /// Weak run constrains add run_constrains from the host env.
    pub(super) weak_constrains: Vec<Dependency>,
}

impl RunExports {
    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constrains.is_empty()
            && self.weak.is_empty()
            && self.weak_constrains.is_empty()
    }

    /// Get all run exports from all configurations
    pub fn all(&self) -> impl Iterator<Item = &Dependency> {
        self.noarch
            .iter()
            .chain(self.strong.iter())
            .chain(self.strong_constrains.iter())
            .chain(self.weak.iter())
            .chain(self.weak_constrains.iter())
    }

    /// Get the noarch run exports.
    pub fn noarch(&self) -> &[Dependency] {
        self.noarch.as_slice()
    }

    /// Get the strong run exports.
    pub fn strong(&self) -> &[Dependency] {
        self.strong.as_slice()
    }

    /// Get the strong run constrains.
    pub fn strong_constrains(&self) -> &[Dependency] {
        self.strong_constrains.as_slice()
    }

    /// Get the weak run exports.
    pub fn weak(&self) -> &[Dependency] {
        self.weak.as_slice()
    }

    /// Get the weak run constrains.
    pub fn weak_constrains(&self) -> &[Dependency] {
        self.weak_constrains.as_slice()
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
                    ErrorKind::InvalidField(invalid.to_owned().into()),
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
