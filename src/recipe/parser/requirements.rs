//! Parsing for the requirements section of the recipe.

use std::{fmt, str::FromStr};

use rattler_conda_types::MatchSpec;
use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub build: Vec<Dependency>,
    /// Requirements at _host_ time are requirements that the final executable is going
    /// to _link_ against. The environment will be resolved with the target_platform
    /// architecture (e.g. if you build _on_ linux-64 _for_ linux-aarch64, then the
    /// host environment will be resolved with linux-aarch64).
    ///
    /// Typically things like libraries, headers, etc. are installed here.
    #[serde(default)]
    pub host: Vec<Dependency>,
    /// Requirements at _run_ time are requirements that the final executable is going
    /// to _run_ against. The environment will be resolved with the target_platform
    /// at runtime.
    #[serde(default)]
    pub run: Vec<Dependency>,
    /// Constrains are optional runtime requirements that are used to constrain the
    /// environment that is resolved. They are not installed by default, but when
    /// installed they will have to conform to the constrains specified here.
    #[serde(default)]
    pub run_constrained: Vec<Dependency>,
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

    /// Get the run constrained requirements.
    pub fn run_constrained(&self) -> &[Dependency] {
        self.run_constrained.as_slice()
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
            .chain(self.run_constrained.iter())
    }

    /// Check if all requirements are empty.
    pub fn is_empty(&self) -> bool {
        self.build.is_empty()
            && self.host.is_empty()
            && self.run.is_empty()
            && self.run_constrained.is_empty()
    }
}

impl TryConvertNode<Requirements> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Requirements, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedMapping,
                    label = format!("expected a mapping for `{name}`")
                )
            })
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Requirements> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<Requirements, PartialParsingError> {
        let mut build = Vec::new();
        let mut host = Vec::new();
        let mut run = Vec::new();
        let mut run_constrained = Vec::new();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "build" => build = value.try_convert(key_str)?,
                "host" => host = value.try_convert(key_str)?,
                "run" => run = value.try_convert(key_str)?,
                "run_constrained" => run_constrained = value.try_convert(key_str)?,
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_string().into()),
                    ))
                }
            }
        }

        Ok(Requirements {
            build,
            host,
            run,
            run_constrained,
        })
    }
}

/// A pin subpackage is a special kind of dependency that is used to depend on
/// another output (subpackage) of the same recipe. The pin is used to specify
/// the version range to pin the subpackage to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinSubpackage {
    /// The pin value.
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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

impl Serialize for Compiler {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        format!("__COMPILER {}", self.language).serialize(serializer)
    }
}

impl FromStr for Compiler {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(lang) = s.strip_prefix("__COMPILER ") {
            Ok(Self {
                language: lang.into(),
            })
        } else {
            Err(format!("compiler without prefix: {}", s))
        }
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
    fn try_convert(&self, name: &str) -> Result<Vec<Dependency>, PartialParsingError> {
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
            RenderedNode::Mapping(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = "expected scalar or sequence"
            )),
            RenderedNode::Null(_) => Ok(vec![]),
        }
    }
}

impl TryConvertNode<Dependency> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<Dependency, PartialParsingError> {
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
        struct DependencyVisitor;

        impl<'de> serde::de::Visitor<'de> for DependencyVisitor {
            type Value = Dependency;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(
                    "a string starting with '__COMPILER', '__PIN_SUBPACKAGE', or a MatchSpec",
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Dependency, E>
            where
                E: serde::de::Error,
            {
                if let Some(compiler_language) = value.strip_prefix("__COMPILER ") {
                    Ok(Dependency::Compiler(Compiler {
                        language: compiler_language.to_lowercase(),
                    }))
                } else if let Some(pin) = value.strip_prefix("__PIN_SUBPACKAGE ") {
                    Ok(Dependency::PinSubpackage(PinSubpackage {
                        pin_subpackage: Pin::from_internal_repr(pin),
                    }))
                } else if let Some(pin) = value.strip_prefix("__PIN_COMPATIBLE ") {
                    Ok(Dependency::PinCompatible(PinCompatible {
                        pin_compatible: Pin::from_internal_repr(pin),
                    }))
                } else {
                    // Assuming MatchSpec can be constructed from a string.
                    MatchSpec::from_str(value)
                        .map(Dependency::Spec)
                        .map_err(serde::de::Error::custom)
                }
            }
        }

        deserializer.deserialize_str(DependencyVisitor)
    }
}

impl Serialize for Dependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        match self {
            Dependency::Spec(spec) => serializer.serialize_str(&spec.to_string()),
            Dependency::PinSubpackage(pin) => serializer.serialize_str(&format!(
                "__PIN_SUBPACKAGE {}",
                pin.pin_subpackage.internal_repr()
            )),
            Dependency::PinCompatible(pin) => serializer.serialize_str(&format!(
                "__PIN_COMPATIBLE {}",
                pin.pin_compatible.internal_repr()
            )),
            Dependency::Compiler(compiler) => {
                serializer.serialize_str(&format!("__COMPILER {}", compiler.language()))
            }
        }
    }
}

impl TryConvertNode<MatchSpec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<MatchSpec, PartialParsingError> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )
            })
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<MatchSpec> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<MatchSpec, PartialParsingError> {
        MatchSpec::from_str(self.as_str()).map_err(|err| {
            _partialerror!(
                *self.span(),
                ErrorKind::from(err),
                label = format!("error parsing `{name}` as a match spec")
            )
        })
    }
}

#[cfg(test)]
mod test {
    use requirements::{Dependency, Requirements};

    use crate::recipe::parser::requirements;

    /// test serialization and deserialization of Compiler
    use super::Compiler;

    #[test]
    fn test_compiler_serde() {
        let compiler = Compiler {
            language: "gcc".to_string(),
        };

        let serialized = serde_yaml::to_string(&compiler).unwrap();
        assert_eq!(serialized, "__COMPILER gcc\n");

        let requirements = Requirements {
            build: vec![Dependency::Compiler(compiler)],
            host: vec![],
            run: vec![],
            run_constrained: vec![],
        };

        insta::assert_yaml_snapshot!(requirements);

        let yaml = serde_yaml::to_string(&requirements).unwrap();
        assert_eq!(
            yaml,
            "build:\n- __COMPILER gcc\nhost: []\nrun: []\nrun_constrained: []\n"
        );

        let deserialized: Requirements = serde_yaml::from_str(&yaml).unwrap();
        insta::assert_yaml_snapshot!(deserialized);
    }
}
