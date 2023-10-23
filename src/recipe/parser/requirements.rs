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
            match key.as_str() {
                "build" => build = value.try_convert("build")?,
                "host" => host = value.try_convert("host")?,
                "run" => run = value.try_convert("run")?,
                "run_constrained" => run_constrained = value.try_convert("run_constrained")?,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinSubpackage {
    pin_subpackage: Pin,
}

impl PinSubpackage {
    /// Get the [`Pin`] value.
    pub const fn pin_value(&self) -> &Pin {
        &self.pin_subpackage
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Compiler {
    compiler: String,
}

impl Compiler {
    /// Get the compiler value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.compiler
    }

    /// Get the compiler value without the `__COMPILER` prefix.
    pub fn without_prefix(&self) -> &str {
        self.compiler
            .strip_prefix("__COMPILER ")
            .expect("compiler without prefix")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Dependency {
    #[serde(deserialize_with = "deserialize_match_spec")]
    Spec(MatchSpec),
    PinSubpackage(PinSubpackage),
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
            let compiler = self.try_convert(name)?;
            Ok(Dependency::Compiler(Compiler { compiler }))
        } else if self.contains("__PIN_SUBPACKAGE") {
            let pin_subpackage: String = self.try_convert(name)?;

            // Panic should never happen from this strip unless the prefix magic for the pin
            // subpackage changes
            let internal_repr = pin_subpackage
                .strip_prefix("__PIN_SUBPACKAGE ")
                .expect("pin subpackage without prefix __PIN_SUBPACKAGE ");
            let pin_subpackage = Pin::from_internal_repr(internal_repr);
            Ok(Dependency::PinSubpackage(PinSubpackage { pin_subpackage }))
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
                if let Some(compiler) = value.strip_prefix("__COMPILER ") {
                    Ok(Dependency::Compiler(Compiler {
                        compiler: compiler.to_lowercase(),
                    }))
                } else if let Some(pin) = value.strip_prefix("__PIN_SUBPACKAGE ") {
                    Ok(Dependency::PinSubpackage(PinSubpackage {
                        pin_subpackage: Pin::from_internal_repr(pin),
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
