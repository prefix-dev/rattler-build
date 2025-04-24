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

use super::Recipe;

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

impl Recipe {
    /// Retrieve all build time requirements, including those from the cache.
    pub fn build_time_requirements(&self) -> Box<dyn Iterator<Item = &Dependency> + '_> {
        if let Some(cache) = self.cache.as_ref() {
            Box::new(
                cache
                    .requirements
                    .build_time()
                    .chain(self.requirements.build_time()),
            )
        } else {
            Box::new(self.requirements.build_time())
        }
    }
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
    pub fn ignore_run_exports(&self, merge: Option<&IgnoreRunExports>) -> IgnoreRunExports {
        let mut ignore = self.ignore_run_exports.clone();
        if let Some(merge) = merge {
            ignore.by_name.extend(merge.by_name.iter().cloned());
            ignore
                .from_package
                .extend(merge.from_package.iter().cloned());
        }
        ignore
    }

    /// Get all requirements at build time (combines build and host requirements)
    pub fn build_time(&self) -> impl Iterator<Item = &Dependency> {
        self.build.iter().chain(self.host.iter())
    }

    /// Get all pin_subpackage expressions from requirements, constraints and run exports
    pub fn all_pin_subpackage(&self) -> impl Iterator<Item = &Pin> {
        self.all_requirements().filter_map(|dep| match dep {
            Dependency::PinSubpackage(pin) => Some(&pin.pin_subpackage),
            _ => None,
        })
    }

    /// Return all dependencies including any constraints, run exports
    /// This is mainly used to find any pin expressions that need to be resolved or added as requirements
    pub fn all_requirements(&self) -> impl Iterator<Item = &Dependency> {
        self.build
            .iter()
            .chain(self.host.iter())
            .chain(self.run.iter())
            .chain(self.run_constraints.iter())
            .chain(self.run_exports.weak.iter())
            .chain(self.run_exports.weak_constraints.iter())
            .chain(self.run_exports.strong.iter())
            .chain(self.run_exports.strong_constraints.iter())
            .chain(self.run_exports.noarch.iter())
    }

    /// Get all requirements in one iterator.
    pub fn run_build_host(&self) -> impl Iterator<Item = &Dependency> {
        self.build
            .iter()
            .chain(self.host.iter())
            .chain(self.run.iter())
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
    pub pin_subpackage: Pin,
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
    pub pin_compatible: Pin,
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
pub struct Language(String);

impl Language {
    /// Get the language value as a string.
    pub fn language(&self) -> &str {
        &self.0
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
                label = format!("expected scalar or sequence for `{name}`")
            )]),
            RenderedNode::Null(_) => Ok(vec![]),
        }
    }
}

impl TryConvertNode<Dependency> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<Dependency, Vec<PartialParsingError>> {
        // Pin subpackage and pin compatible are serialized into JSON by the `jinja` converter
        if self.starts_with('{') {
            // try to convert from a YAML dictionary
            let dependency: Dependency =
                serde_yaml::from_str(self.as_str()).expect("Internal repr error");
            Ok(dependency)
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
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        #[allow(clippy::large_enum_variant)]
        enum RawSpec {
            String(String),
            Explicit(#[serde(with = "serde_yaml::with::singleton_map")] RawDependency),
        }

        let raw_spec = RawSpec::deserialize(deserializer)?;
        Ok(match raw_spec {
            RawSpec::String(spec) => Dependency::Spec(spec.parse().map_err(D::Error::custom)?),
            RawSpec::Explicit(RawDependency::PinSubpackage(dep)) => Dependency::PinSubpackage(dep),
            RawSpec::Explicit(RawDependency::PinCompatible(dep)) => Dependency::PinCompatible(dep),
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
    fn try_convert(&self, _name: &str) -> Result<MatchSpec, Vec<PartialParsingError>> {
        let string = self.as_str();

        // if we have a matchspec that is only numbers, and ., we complain and ask the user to add a
        // `.*` or `==` in front of it.
        let split_string = string.split_whitespace().collect::<Vec<_>>();
        if split_string.len() >= 2 && split_string[1].chars().all(|c| c.is_numeric() || c == '.') {
            let name = split_string[0];
            let version = split_string[1];
            let rest = split_string[2..].join(" ");
            let rest = if rest.is_empty() {
                "".to_string()
            } else {
                format!(" {}", rest)
            };

            return Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = format!(
                    "This match spec is ambiguous. Do you mean `{name} =={version}{rest}` or `{name} {version}.*{rest}`?"
                )
            )]);
        }

        MatchSpec::from_str(self.as_str(), ParseStrictness::Strict).map_err(|err| {
            let str = self.as_str();
            vec![_partialerror!(
                *self.span(),
                ErrorKind::from(err),
                label = format!("error parsing `{str}` as a match spec")
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
            let deps: Vec<Dependency> = node.try_convert(name)?;
            run_exports.weak.extend(deps);
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
    /// Run exports to ignore by name of the package that is exported
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    pub by_name: IndexSet<PackageName>,
    /// Run exports to ignore by the package that applies them
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    pub from_package: IndexSet<PackageName>,
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

impl TryConvertNode<IgnoreRunExports> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<IgnoreRunExports, Vec<PartialParsingError>> {
        let mut ignore_run_exports = IgnoreRunExports::default();
        let mut errors = vec![];

        let known_fields = std::collections::HashSet::from(["by_name", "from_package"]);
        for (key, _value) in self.iter() {
            let key_str = key.as_str();
            if !known_fields.contains(key_str) {
                errors.push(_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(key_str.to_string().into()),
                    help = "valid fields for `ignore_run_exports` are: `by_name`, `from_package`"
                ));
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        if let Some(by_name_node) = self.get("by_name") {
            match TryConvertNode::<Vec<MatchSpec>>::try_convert(by_name_node, "by_name") {
                Ok(specs) => {
                    ignore_run_exports.by_name = specs
                        .into_iter()
                        .map(|ms: MatchSpec| ms.name.expect("MatchSpec must have a name"))
                        .collect();
                }
                Err(e) => errors.extend(e),
            }
        }

        if let Some(from_package_node) = self.get("from_package") {
            match TryConvertNode::<Vec<MatchSpec>>::try_convert(from_package_node, "from_package") {
                Ok(specs) => {
                    ignore_run_exports.from_package = specs
                        .into_iter()
                        .map(|ms: MatchSpec| ms.name.expect("MatchSpec must have a name"))
                        .collect();
                }
                Err(e) => errors.extend(e),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(ignore_run_exports)
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use crate::render::pin::PinArgs;

    use super::*;

    #[test]
    fn test_pin_package() {
        let pin_subpackage = PinSubpackage {
            pin_subpackage: Pin {
                name: PackageName::from_str("foo").unwrap(),
                args: PinArgs {
                    lower_bound: Some("x.x.x.x".parse().unwrap()),
                    upper_bound: Some("x.x".parse().unwrap()),
                    ..Default::default()
                },
            },
        };

        let pin_compatible = PinCompatible {
            pin_compatible: Pin {
                name: PackageName::from_str("bar").unwrap(),
                args: PinArgs {
                    lower_bound: Some("x.x".parse().unwrap()),
                    upper_bound: Some("x.x.x".parse().unwrap()),
                    ..Default::default()
                },
            },
        };

        let pin_compatible_2 = PinCompatible {
            pin_compatible: Pin {
                name: PackageName::from_str("bar").unwrap(),
                args: PinArgs {
                    lower_bound: Some("x.x".parse().unwrap()),
                    upper_bound: None,
                    exact: true,
                    ..Default::default()
                },
            },
        };

        let spec = MatchSpec::from_str("foo >=3.1", ParseStrictness::Strict).unwrap();

        let requirements = Requirements {
            build: vec![
                Dependency::Spec(spec),
                Dependency::PinSubpackage(pin_subpackage),
                Dependency::PinCompatible(pin_compatible),
                Dependency::PinCompatible(pin_compatible_2),
            ],
            ..Default::default()
        };

        insta::assert_snapshot!(serde_yaml::to_string(&requirements).unwrap());
    }

    #[test]
    fn test_deserialize_pin() {
        let pin = "{ pin_subpackage: { name: foo, upper_bound: x.x.x, lower_bound: x.x, exact: true, spec: foo }}";
        let _: Dependency = serde_yaml::from_str(pin).unwrap();
    }
}
