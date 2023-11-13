use std::{collections::BTreeMap, str::FromStr};

use rattler_conda_types::{package::EntryPoint, NoArchKind, NoArchType, PackageName};
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
            TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
};

use super::Dependency;

/// The build options contain information about how to build the package and some additional
/// metadata about the package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Build {
    /// The build number is a number that should be incremented every time the recipe is built.
    pub(super) number: u64,
    /// The build string is usually set automatically as the hash of the variant configuration.
    /// It's possible to override this by setting it manually, but not recommended.
    pub(super) string: Option<String>,
    /// List of conditions under which to skip the build of the package.
    pub(super) skip: bool,
    /// The build script can be either a list of commands or a path to a script. By
    /// default, the build script is set to `build.sh` or `build.bat` on Unix and Windows respectively.
    pub(super) script: Vec<String>,
    /// Environment variables to pass through or set in the script
    pub(super) script_env: ScriptEnv,
    /// A recipe can choose to ignore certain run exports of its dependencies
    pub(super) ignore_run_exports: Vec<PackageName>,
    /// A recipe can choose to ignore all run exports of coming from some packages
    pub(super) ignore_run_exports_from: Vec<PackageName>,
    /// The recipe can specify a list of run exports that it provides
    pub(super) run_exports: RunExports,
    /// A noarch package runs on any platform. It can be either a python package or a generic package.
    pub(super) noarch: NoArchType,
    /// For a Python noarch package to have executables it is necessary to specify the python entry points.
    /// These contain the name of the executable and the module + function that should be executed.
    pub(super) entry_points: Vec<EntryPoint>,
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
    pub fn scripts(&self) -> &[String] {
        self.script.as_slice()
    }

    /// Get the build script environment.
    pub const fn script_env(&self) -> &ScriptEnv {
        &self.script_env
    }

    /// Get run exports.
    pub const fn run_exports(&self) -> &RunExports {
        &self.run_exports
    }

    /// Get the ignore run exports.
    ///
    /// A recipe can choose to ignore certain run exports of its dependencies
    pub fn ignore_run_exports(&self) -> &[PackageName] {
        self.ignore_run_exports.as_slice()
    }

    /// Get the ignore run exports from.
    ///
    /// A recipe can choose to ignore all run exports of coming from some packages
    pub fn ignore_run_exports_from(&self) -> &[PackageName] {
        self.ignore_run_exports_from.as_slice()
    }

    /// Get the noarch type.
    pub const fn noarch(&self) -> &NoArchType {
        &self.noarch
    }

    /// Get the entry points.
    pub fn entry_points(&self) -> &[EntryPoint] {
        self.entry_points.as_slice()
    }

    /// Check if the build should be skipped.
    pub fn is_skip_build(&self) -> bool {
        self.skip()
    }
}

impl TryConvertNode<Build> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Build, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping))
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Build> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<Build, PartialParsingError> {
        let mut build = Build::default();

        for (key, value) in self.iter() {
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
                "script_env" => build.script_env = value.try_convert(key_str)?,
                "ignore_run_exports" => {
                    build.ignore_run_exports = value.try_convert(key_str)?;
                }
                "ignore_run_exports_from" => {
                    build.ignore_run_exports_from = value.try_convert(key_str)?;
                }
                "noarch" => {
                    build.noarch = value.try_convert(key_str)?;
                }
                "run_exports" => {
                    build.run_exports = value.try_convert(key_str)?;
                }
                "entry_points" => {
                    if let Some(NoArchKind::Generic) = build.noarch.kind() {
                        return Err(_partialerror!(
                            *key.span(),
                            ErrorKind::Other,
                            label = "`entry_points` are only allowed for `python` noarch packages"
                        ));
                    }

                    build.entry_points = value.try_convert(key_str)?;
                }
                invalid => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                    ))
                }
            }
        }

        Ok(build)
    }
}

/// Extra environment variables to set during the build script execution
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScriptEnv {
    /// Environments variables to leak into the build environment from the host system.
    /// During build time these variables are recorded and stored in the package output.
    /// Use `secrets` for environment variables that should not be recorded.
    pub(super) passthrough: Vec<String>,
    /// Environment variables to set in the build environment.
    pub(super) env: BTreeMap<String, String>,
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information. Use with care because this might make recipes no
    /// longer reproducible on other machines.
    pub(super) secrets: Vec<String>,
}

impl ScriptEnv {
    /// Check if the script environment is empty is all its fields.
    pub fn is_empty(&self) -> bool {
        self.passthrough.is_empty() && self.env.is_empty() && self.secrets.is_empty()
    }

    /// Get the passthrough environment variables.
    ///
    /// Those are the environments variables to leak into the build environment from the host system.
    ///
    /// During build time these variables are recorded and stored in the package output.
    /// Use `secrets` for environment variables that should not be recorded.
    pub fn passthrough(&self) -> &[String] {
        self.passthrough.as_slice()
    }

    /// Get the environment variables to set in the build environment.
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Get the secrets environment variables.
    ///
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information.
    ///
    /// # Warning
    /// Use with care because this might make recipes no longer reproducible on other machines.
    pub fn secrets(&self) -> &[String] {
        self.secrets.as_slice()
    }
}

impl TryConvertNode<ScriptEnv> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<ScriptEnv, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping))
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<ScriptEnv> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<ScriptEnv, PartialParsingError> {
        let invalid = self
            .keys()
            .find(|k| matches!(k.as_str(), "env" | "passthrough" | "secrets"));

        if let Some(invalid) = invalid {
            return Err(_partialerror!(
                *invalid.span(),
                ErrorKind::InvalidField(invalid.to_string().into()),
                help = format!("valid keys for {name} are `env`, `passthrough` or `secrets`")
            ));
        }

        let env = self
            .get("env")
            .map(|node| node.try_convert("env"))
            .transpose()?
            .unwrap_or_default();

        let passthrough = self
            .get("passthrough")
            .map(|node| node.try_convert("passthrough"))
            .transpose()?
            .unwrap_or_default();

        let secrets = self
            .get("secrets")
            .map(|node| node.try_convert("secrets"))
            .transpose()?
            .unwrap_or_default();

        Ok(ScriptEnv {
            passthrough,
            env,
            secrets,
        })
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

impl TryConvertNode<RunExports> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, PartialParsingError> {
        match self {
            RenderedNode::Scalar(s) => s.try_convert(name),
            RenderedNode::Sequence(seq) => seq.try_convert(name),
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Null(_) => Ok(RunExports::default()),
        }
    }
}

impl TryConvertNode<RunExports> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, PartialParsingError> {
        let mut run_exports = RunExports::default();

        let dep = self.try_convert(name)?;
        run_exports.weak.push(dep);

        Ok(run_exports)
    }
}

impl TryConvertNode<RunExports> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, PartialParsingError> {
        let mut run_exports = RunExports::default();

        for node in self.iter() {
            let deps = node.try_convert(name)?;
            run_exports.weak = deps;
        }

        Ok(run_exports)
    }
}

impl TryConvertNode<RunExports> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, PartialParsingError> {
        let mut run_exports = RunExports::default();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "noarch" => {
                    run_exports.noarch = value.try_convert(key_str)?;
                }
                "strong" => {
                    let deps = value.try_convert(key_str)?;
                    run_exports.strong = deps;
                }
                "strong_constrains" => {
                    let deps = value.try_convert(key_str)?;
                    run_exports.strong_constrains = deps;
                }
                "weak" => {
                    let deps = value.try_convert(key_str)?;
                    run_exports.weak = deps;
                }
                "weak_constrains" => {
                    let deps = value.try_convert(key_str)?;
                    run_exports.weak_constrains = deps;
                }
                invalid => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_owned().into()),
                        help = format!("fields for {name} should be one of: `weak`, `strong`, `noarch`, `strong_constrains`, or `weak_constrains`")
                    ))
                }
            }
        }

        Ok(run_exports)
    }
}

impl TryConvertNode<NoArchType> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<NoArchType, PartialParsingError> {
        self.as_scalar()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedScalar,))?
            .try_convert(name)
    }
}

impl TryConvertNode<NoArchType> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<NoArchType, PartialParsingError> {
        let noarch = self.as_str();
        let noarch = match noarch {
            "python" => NoArchType::python(),
            "generic" => NoArchType::generic(),
            invalid => {
                return Err(_partialerror!(
                    *self.span(),
                    ErrorKind::InvalidField(invalid.to_owned().into()),
                    help = format!("expected `python` or `generic` for {name}"),
                ))
            }
        };
        Ok(noarch)
    }
}

impl TryConvertNode<EntryPoint> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<EntryPoint, PartialParsingError> {
        self.as_scalar()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedScalar))
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<EntryPoint> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<EntryPoint, PartialParsingError> {
        EntryPoint::from_str(self.as_str())
            .map_err(|err| _partialerror!(*self.span(), ErrorKind::EntryPointParsing(err),))
    }
}
