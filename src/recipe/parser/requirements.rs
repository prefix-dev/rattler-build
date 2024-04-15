//! Parsing for the requirements section of the recipe.

use crate::recipe::parser::FlattenErrors;
use indexmap::IndexSet;
use rattler_conda_types::{MatchSpec, PackageName, ParseStrictness};
use serde::de::Error;
use serde::{Deserialize, Serialize};

use crate::recipe::custom_yaml::RenderedSequenceNode;
use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
    render::pin::Pin,
};

/// The requirements at build- and runtime are defined in the `requirements` section of the recipe.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Requirements {
    /// Requirements at _build_ time are requirements that can
    /// be run on the machine that is executing the build script.
    /// The environment will thus be resolved with the appropriate platform
    /// that is currently running (e.g. on linux-64 it will be resolved with linux-64).
    /// Typically things like compilers, build tools, etc. are installed here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<Dependency>,
    /// Requirements at _host_ time are requirements that the final executable is going
    /// to _link_ against. The environment will be resolved with the target_platform
    /// architecture (e.g. if you build _on_ linux-64 _for_ linux-aarch64, then the
    /// host environment will be resolved with linux-aarch64).
    ///
    /// Typically things like libraries, headers, etc. are installed here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<Dependency>,
    /// Requirements at _run_ time are requirements that the final executable is going
    /// to _run_ against. The environment will be resolved with the target_platform
    /// at runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<Dependency>,
    /// Constrains are optional runtime requirements that are used to constrain the
    /// environment that is resolved. They are not installed by default, but when
    /// installed they will have to conform to the constrains specified here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_constraints: Vec<Dependency>,

    /// The recipe can specify a list of run exports that it provides.
    #[serde(default, skip_serializing_if = "RunExports::is_empty")]
    pub run_exports: RunExports,

    /// Ignore run-exports by name or from certain packages
    #[serde(default, skip_serializing_if = "IgnoreRunExports::is_empty")]
    pub ignore_run_exports: IgnoreRunExports,
}

impl Requirements {
    /// Get the build requirements.
    pub fn build(&self) -> &[Dependency] {
        self.build.as_slice()
    }

    /// Get the host requirements.
    pub fn host(&self) -> &[Dependency] {
        self.host.as_slice()
    }

    /// Get the run requirements.
    pub fn run(&self) -> &[Dependency] {
        self.run.as_slice()
    }

    /// Get the run constraints requirements.
    pub fn run_constraints(&self) -> &[Dependency] {
        self.run_constraints.as_slice()
    }

    /// Get run exports.
    pub const fn run_exports(&self) -> &RunExports {
        &self.run_exports
    }

    /// Get run exports that are ignored.
    pub const fn ignore_run_exports(&self) -> &IgnoreRunExports {
        &self.ignore_run_exports
    }

    /// Get all requirements at build time (combines build and host requirements)
    pub fn build_time(&self) -> impl Iterator<Item = &Dependency> {
        self.build.iter().chain(self.host.iter())
    }

    /// Get all requirements in one iterator.
    pub fn all(&self) -> impl Iterator<Item = &Dependency> {
        self.build
            .iter()
            .chain(self.host.iter())
            .chain(self.run.iter())
            .chain(self.run_constraints.iter())
    }

    /// Check if all requirements are empty.
    pub fn is_empty(&self) -> bool {
        self.build.is_empty()
            && self.host.is_empty()
            && self.run.is_empty()
            && self.run_constraints.is_empty()
    }
}

impl TryConvertNode<Requirements> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Requirements, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| {
                vec![_partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedMapping,
                    label = format!("expected a mapping for `{name}`")
                )]
            })
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Requirements> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<Requirements, Vec<PartialParsingError>> {
        let mut requirements = Requirements::default();

        crate::validate_keys!(
            requirements,
            self.iter(),
            build,
            host,
            run,
            run_constraints,
            run_exports,
            ignore_run_exports
        );

        Ok(requirements)
    }
}

/// A pin subpackage is a special kind of dependency that is used to depend on
/// another output (subpackage) of the same recipe. The pin is used to specify
/// the version range to pin the subpackage to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinSubpackage {
    /// The pin value.
    #[serde(flatten)]
    pin_subpackage: Pin,
}

impl PinSubpackage {
    /// Get the [`Pin`] value.
    pub const fn pin_value(&self) -> &Pin {
        &self.pin_subpackage
    }
}

/// A pin compatible is a special kind of dependency that is used to depend on
/// a package from a previously resolved environment and applies a version range
/// to the resolved version (e.g. resolve `python` in the host env to 3.10 and pin in the run
/// env to `>=3.10,<3.11`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinCompatible {
    /// The pin value.
    #[serde(flatten)]
    pin_compatible: Pin,
}

impl PinCompatible {
    /// Get the [`Pin`] value.
    pub const fn pin_value(&self) -> &Pin {
        &self.pin_compatible
    }
}

/// A compiler is a special kind of dependency that, when rendered, has
/// some additional information about the target_platform attached.
///
/// For example, a c-compiler will resolve to the variant key `c_compiler`.
/// If that value is `gcc`, the rendered compiler will read `gcc_linux-64` because
/// it is always resolved with the target_platform.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Compiler {
    /// The language such as c, cxx, rust, etc.
    language: String,
}

impl Compiler {
    /// Get the language value as a string.
    pub fn language(&self) -> &str {
        &self.language
    }
}

/// A combination of all possible dependencies.
#[derive(Debug, Clone)]
pub enum Dependency {
    /// A regular matchspec
    Spec(MatchSpec),
    /// A pin_subpackage dependency
    PinSubpackage(PinSubpackage),
    /// A pin_compatible dependency
    PinCompatible(PinCompatible),
    /// A compiler dependency
    Compiler(Compiler),
}

impl TryConvertNode<Vec<Dependency>> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Vec<Dependency>, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Scalar(s) => {
                let dep: Dependency = s.try_convert(name)?;
                Ok(vec![dep])
            }
            RenderedNode::Sequence(seq) => {
                let mut deps = Vec::new();
                for n in seq.iter() {
                    let n_deps: Vec<_> = n.try_convert(name)?;
                    deps.extend(n_deps);
                }
                Ok(deps)
            }
            RenderedNode::Mapping(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = "expected scalar or sequence"
            )]),
            RenderedNode::Null(_) => Ok(vec![]),
        }
    }
}

impl TryConvertNode<Dependency> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<Dependency, Vec<PartialParsingError>> {
        // compiler
        if self.contains("__COMPILER") {
            let compiler: String = self.try_convert(name)?;
            let language = compiler
                .strip_prefix("__COMPILER ")
                .expect("compiler without prefix");
            // Panic should never happen from this strip unless the prefix magic for the compiler
            Ok(Dependency::Compiler(Compiler {
                language: language.to_string(),
            }))
        } else if self.contains("__PIN_SUBPACKAGE") {
            let pin_subpackage: String = self.try_convert(name)?;

            // Panic should never happen from this strip unless the
            // prefix magic for the pin subpackage changes
            let internal_repr = pin_subpackage
                .strip_prefix("__PIN_SUBPACKAGE ")
                .expect("pin subpackage without prefix __PIN_SUBPACKAGE ");
            let pin_subpackage = Pin::from_internal_repr(internal_repr);
            Ok(Dependency::PinSubpackage(PinSubpackage { pin_subpackage }))
        } else if self.contains("__PIN_COMPATIBLE") {
            let pin_compatible: String = self.try_convert(name)?;

            // Panic should never happen from this strip unless the
            // prefix magic for the pin compatible changes
            let internal_repr = pin_compatible
                .strip_prefix("__PIN_COMPATIBLE ")
                .expect("pin compatible without prefix __PIN_COMPATIBLE ");
            let pin_compatible = Pin::from_internal_repr(internal_repr);
            Ok(Dependency::PinCompatible(PinCompatible { pin_compatible }))
        } else {
            let spec = self.try_convert(name)?;

            Ok(Dependency::Spec(spec))
        }
    }
}

impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum RawDependency {
            PinSubpackage(PinSubpackage),
            PinCompatible(PinCompatible),
            Compiler(Compiler),
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawSpec {
            String(String),
            Explicit(#[serde(with = "serde_yaml::with::singleton_map")] RawDependency),
        }

        let raw_spec = RawSpec::deserialize(deserializer)?;
        Ok(match raw_spec {
            RawSpec::String(spec) => Dependency::Spec(spec.parse().map_err(D::Error::custom)?),
            RawSpec::Explicit(RawDependency::PinSubpackage(dep)) => Dependency::PinSubpackage(dep),
            RawSpec::Explicit(RawDependency::PinCompatible(dep)) => Dependency::PinCompatible(dep),
            RawSpec::Explicit(RawDependency::Compiler(dep)) => Dependency::Compiler(dep),
        })
    }
}

impl Serialize for Dependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum RawDependency<'a> {
            PinSubpackage(&'a PinSubpackage),
            PinCompatible(&'a PinCompatible),
            Compiler(&'a Compiler),
        }

        #[derive(Serialize)]
        #[serde(untagged)]
        enum RawSpec<'a> {
            String(String),
            Explicit(#[serde(with = "serde_yaml::with::singleton_map")] RawDependency<'a>),
        }

        let raw = match self {
            Dependency::Spec(dep) => RawSpec::String(dep.to_string()),
            Dependency::PinSubpackage(dep) => RawSpec::Explicit(RawDependency::PinSubpackage(dep)),
            Dependency::PinCompatible(dep) => RawSpec::Explicit(RawDependency::PinCompatible(dep)),
            Dependency::Compiler(dep) => RawSpec::Explicit(RawDependency::Compiler(dep)),
        };

        raw.serialize(serializer)
    }
}

impl TryConvertNode<MatchSpec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<MatchSpec, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                vec![_partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )]
            })
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<MatchSpec> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<MatchSpec, Vec<PartialParsingError>> {
        MatchSpec::from_str(self.as_str(), ParseStrictness::Strict).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::from(err),
                label = format!("error parsing `{name}` as a match spec")
            )]
        })
    }
}
/// Run exports are applied to downstream packages that depend on this package.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RunExports {
    /// Noarch run exports are the only ones looked at when building noarch packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noarch: Vec<Dependency>,
    /// Strong run exports apply from the build and host env to the run env.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strong: Vec<Dependency>,
    /// Strong run constrains add run_constrains from the build and host env.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strong_constraints: Vec<Dependency>,
    /// Weak run exports apply from the host env to the run env.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weak: Vec<Dependency>,
    /// Weak run constrains add run_constrains from the host env.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

    /// Get the noarch run exports.
    pub fn noarch(&self) -> &[Dependency] {
        self.noarch.as_slice()
    }

    /// Get the strong run exports.
    pub fn strong(&self) -> &[Dependency] {
        self.strong.as_slice()
    }

    /// Get the strong run constrains.
    pub fn strong_constraints(&self) -> &[Dependency] {
        self.strong_constraints.as_slice()
    }

    /// Get the weak run exports.
    pub fn weak(&self) -> &[Dependency] {
        self.weak.as_slice()
    }

    /// Get the weak run constrains.
    pub fn weak_constraints(&self) -> &[Dependency] {
        self.weak_constraints.as_slice()
    }
}

impl TryConvertNode<RunExports> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Scalar(s) => s.try_convert(name),
            RenderedNode::Sequence(seq) => seq.try_convert(name),
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Null(_) => Ok(RunExports::default()),
        }
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

impl TryConvertNode<RunExports> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<RunExports, Vec<PartialParsingError>> {
        let mut run_exports = RunExports::default();

        for node in self.iter() {
            let deps = node.try_convert(name)?;
            run_exports.weak = deps;
        }

        Ok(run_exports)
    }
}

impl TryConvertNode<RunExports> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<RunExports, Vec<PartialParsingError>> {
        let mut run_exports = RunExports::default();

        crate::validate_keys!(
            run_exports,
            self.iter(),
            noarch,
            strong,
            strong_constraints,
            weak,
            weak_constraints
        );

        Ok(run_exports)
    }
}

/// Run exports to ignore
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IgnoreRunExports {
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    pub(super) by_name: IndexSet<PackageName>,
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    pub(super) from_package: IndexSet<PackageName>,
}

impl IgnoreRunExports {
    /// Returns the package names that should be ignored as run_exports.
    pub fn by_name(&self) -> &IndexSet<PackageName> {
        &self.by_name
    }

    /// Returns the package names from who we should ignore any run_export requirement.
    #[allow(clippy::wrong_self_convention)]
    pub fn from_package(&self) -> &IndexSet<PackageName> {
        &self.from_package
    }

    /// Returns true if this instance is considered empty, e.g. no run_exports should be ignored at
    /// all.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.from_package.is_empty()
    }
}

impl TryConvertNode<IgnoreRunExports> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<IgnoreRunExports, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<IgnoreRunExports> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<IgnoreRunExports, Vec<PartialParsingError>> {
        let mut ignore_run_exports = IgnoreRunExports::default();

        crate::validate_keys!(ignore_run_exports, self.iter(), by_name, from_package);

        Ok(ignore_run_exports)
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use crate::recipe::jinja::PinExpression;

    use super::*;

    #[test]
    fn test_compiler_serde() {
        let compiler = Compiler {
            language: "gcc".to_string(),
        };

        let serialized = serde_yaml::to_string(&compiler).unwrap();
        assert_eq!(serialized, "gcc\n");

        let requirements = Requirements {
            build: vec![Dependency::Compiler(compiler)],
            ..Default::default()
        };

        insta::assert_yaml_snapshot!(requirements);

        let yaml = serde_yaml::to_string(&requirements).unwrap();
        assert_eq!(yaml, "build:\n- compiler: gcc\n");

        let deserialized: Requirements = serde_yaml::from_str(&yaml).unwrap();
        insta::assert_yaml_snapshot!(deserialized);
    }

    #[test]
    fn test_pin_package() {
        let pin_subpackage = PinSubpackage {
            pin_subpackage: Pin {
                name: PackageName::from_str("foo").unwrap(),
                max_pin: Some(PinExpression::from_str("x.x").unwrap()),
                min_pin: Some(PinExpression::from_str("x.x.x.x").unwrap()),
                exact: false,
            },
        };

        let pin_compatible = PinCompatible {
            pin_compatible: Pin {
                name: PackageName::from_str("bar").unwrap(),
                max_pin: Some(PinExpression::from_str("x.x.x").unwrap()),
                min_pin: Some(PinExpression::from_str("x.x").unwrap()),
                exact: false,
            },
        };

        let pin_compatible_2 = PinCompatible {
            pin_compatible: Pin {
                name: PackageName::from_str("bar").unwrap(),
                max_pin: None,
                min_pin: Some(PinExpression::from_str("x.x").unwrap()),
                exact: true,
            },
        };

        let spec = MatchSpec::from_str("foo >=3.1", ParseStrictness::Strict).unwrap();
        let compiler = Compiler {
            language: "gcc".to_string(),
        };

        let requirements = Requirements {
            build: vec![
                Dependency::Spec(spec),
                Dependency::PinSubpackage(pin_subpackage),
                Dependency::PinCompatible(pin_compatible),
                Dependency::PinCompatible(pin_compatible_2),
                Dependency::Compiler(compiler),
            ],
            ..Default::default()
        };

        insta::assert_snapshot!(serde_yaml::to_string(&requirements).unwrap());
    }
}
