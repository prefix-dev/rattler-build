use std::{fmt, str::FromStr};

use rattler_conda_types::MatchSpec;
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, Node, ScalarNode, SequenceNodeInternal},
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1,
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
    pub(super) fn from_stage1(
        req: &stage1::Requirements,
        jinja: &Jinja,
    ) -> Result<Self, PartialParsingError> {
        let build = req
            .build
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());
        let host = req
            .host
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());
        let run = req
            .run
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());
        let run_constrained = req
            .run_constrained
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());

        Ok(Self {
            build,
            host,
            run,
            run_constrained,
        })
    }

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

impl Dependency {
    pub(super) fn from_node(node: &Node, jinja: &Jinja) -> Result<Vec<Self>, PartialParsingError> {
        match node {
            Node::Scalar(s) => {
                let dep = Self::from_scalar(s, jinja)?
                    .map(|d| vec![d])
                    .unwrap_or_default();

                Ok(dep)
            }
            Node::Sequence(seq) => {
                let mut deps = Vec::new();
                for inner in seq.iter() {
                    match inner {
                        SequenceNodeInternal::Simple(n) => deps.extend(Self::from_node(n, jinja)?),
                        SequenceNodeInternal::Conditional(if_sel) => {
                            let if_res = if_sel.process(jinja)?;
                            if let Some(if_res) = if_res {
                                deps.extend(Self::from_node(&if_res, jinja)?)
                            }
                        }
                    }
                }
                Ok(deps)
            }
            Node::Mapping(_) => Err(_partialerror!(
                *node.span(),
                ErrorKind::Other,
                label = "expected scalar or sequence"
            )),
        }
    }

    pub(super) fn from_scalar(
        s: &ScalarNode,
        jinja: &Jinja,
    ) -> Result<Option<Self>, PartialParsingError> {
        // compiler
        if s.as_str().contains("compiler(") {
            let compiler = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering compiler"
                )
            })?;
            Ok(Some(Self::Compiler(Compiler { compiler })))
        } else if s.as_str().contains("pin_subpackage(") {
            let pin_subpackage = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering pin_subpackage"
                )
            })?;

            // Panic should never happen from this strip unless the prefix magic for the pin
            // subpackage changes
            let internal_repr = pin_subpackage
                .strip_prefix("__PIN_SUBPACKAGE ")
                .expect("pin subpackage without prefix __PIN_SUBPACKAGE ");
            let pin_subpackage = Pin::from_internal_repr(internal_repr);
            Ok(Some(Self::PinSubpackage(PinSubpackage { pin_subpackage })))
        } else {
            let spec = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering spec"
                )
            })?;

            if spec.is_empty() {
                return Ok(None);
            }

            let spec = MatchSpec::from_str(&spec).map_err(|_err| {
                _partialerror!(*s.span(), ErrorKind::Other, label = "error parsing spec")
            })?;
            Ok(Some(Self::Spec(spec)))
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
