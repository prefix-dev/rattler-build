//! Stage 1 Requirements - evaluated dependencies with concrete values

use rattler_build_types::Pin;
use rattler_conda_types::{MatchSpec, PackageName};
use serde::{Deserialize, Serialize};

use crate::stage0::evaluate::is_free_matchspec;

/// A pin_subpackage dependency - pins to another output of the same recipe
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinSubpackage {
    /// The pin value
    #[serde(flatten)]
    pub pin_subpackage: Pin,
}

/// A pin_compatible dependency - pins to a compatible version of a package
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinCompatible {
    /// The pin value
    #[serde(flatten)]
    pub pin_compatible: Pin,
}

/// A combination of all possible dependency types
#[derive(Debug, Clone, PartialEq)]
pub enum Dependency {
    /// A regular matchspec dependency
    Spec(Box<MatchSpec>),
    /// A pin_subpackage dependency
    PinSubpackage(PinSubpackage),
    /// A pin_compatible dependency
    PinCompatible(PinCompatible),
}

impl std::fmt::Display for Dependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dependency::Spec(spec) => write!(f, "{}", spec.as_ref()),
            Dependency::PinSubpackage(pin) => {
                write!(
                    f,
                    "pin_subpackage({})",
                    pin.pin_subpackage.name.as_normalized()
                )
            }
            Dependency::PinCompatible(pin) => {
                write!(
                    f,
                    "pin_compatible({})",
                    pin.pin_compatible.name.as_normalized()
                )
            }
        }
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
            Dependency::Spec(dep) => RawSpec::String(dep.as_ref().to_string()),
            Dependency::PinSubpackage(dep) => RawSpec::Explicit(RawDependency::PinSubpackage(dep)),
            Dependency::PinCompatible(dep) => RawSpec::Explicit(RawDependency::PinCompatible(dep)),
        };

        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

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
            RawSpec::String(spec) => {
                Dependency::Spec(Box::new(spec.parse().map_err(D::Error::custom)?))
            }
            RawSpec::Explicit(RawDependency::PinSubpackage(dep)) => Dependency::PinSubpackage(dep),
            RawSpec::Explicit(RawDependency::PinCompatible(dep)) => Dependency::PinCompatible(dep),
        })
    }
}

/// Run exports configuration
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunExports {
    /// Noarch run exports
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noarch: Vec<Dependency>,

    /// Strong run exports (apply from build and host env to run env)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strong: Vec<Dependency>,

    /// Strong run constraints
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strong_constraints: Vec<Dependency>,

    /// Weak run exports (apply from host env to run env)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weak: Vec<Dependency>,

    /// Weak run constraints
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weak_constraints: Vec<Dependency>,
}

impl RunExports {
    /// Create a new empty RunExports
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constraints.is_empty()
            && self.weak.is_empty()
            && self.weak_constraints.is_empty()
    }
}

/// Ignore run exports configuration
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IgnoreRunExports {
    /// Packages to ignore run exports from by name
    /// TODO: move to PackageName perhaps (or spec!?)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_name: Vec<String>,

    /// Packages whose run_exports to ignore
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from_package: Vec<String>,
}

impl IgnoreRunExports {
    /// Create a new empty IgnoreRunExports
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.from_package.is_empty()
    }
}

/// Evaluated requirements with all templates and conditionals resolved
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Requirements {
    /// Build-time dependencies (available during build)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<Dependency>,

    /// Host dependencies (available during build and runtime)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<Dependency>,

    /// Runtime dependencies
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<Dependency>,

    /// Runtime constraints (optional requirements that constrain the environment)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_constraints: Vec<Dependency>,

    /// Run exports configuration
    #[serde(default, skip_serializing_if = "RunExports::is_empty")]
    pub run_exports: RunExports,

    /// Ignore run exports from specific packages
    #[serde(default, skip_serializing_if = "IgnoreRunExports::is_empty")]
    pub ignore_run_exports: IgnoreRunExports,
}

impl Requirements {
    /// Create a new empty Requirements
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the Requirements section is empty
    pub fn is_empty(&self) -> bool {
        self.build.is_empty()
            && self.host.is_empty()
            && self.run.is_empty()
            && self.run_constraints.is_empty()
            && self.run_exports.is_empty()
            && self.ignore_run_exports.is_empty()
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

    /// Get the free specs of the rendered dependencies (any MatchSpec without pins)
    pub fn free_specs(&self) -> Vec<PackageName> {
        self.build
            .iter()
            .chain(self.host.iter())
            .filter_map(|dep| match dep {
                Dependency::Spec(spec) => {
                    if is_free_matchspec(spec.as_ref()) {
                        // is_free_matchspec ensures name is Some
                        Some(spec.name.clone().unwrap())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    }

    /// Get all pin_subpackage dependencies from all requirement sections
    pub fn all_pin_subpackages(&self) -> impl Iterator<Item = &PinSubpackage> {
        self.all_requirements().filter_map(|dep| match dep {
            Dependency::PinSubpackage(pin) => Some(pin),
            _ => None,
        })
    }

    /// Get all pin_subpackage dependencies with exact=true
    pub fn exact_pin_subpackages(&self) -> impl Iterator<Item = &PinSubpackage> {
        self.all_pin_subpackages()
            .filter(|pin| pin.pin_subpackage.args.exact)
    }

    /// Get all pin_compatible dependencies from all requirement sections
    pub fn all_pin_compatible(&self) -> impl Iterator<Item = &PinCompatible> {
        self.all_requirements().filter_map(|dep| match dep {
            Dependency::PinCompatible(pin) => Some(pin),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requirements_creation() {
        let reqs = Requirements::new();
        assert!(reqs.is_empty());
    }

    #[test]
    fn test_requirements_with_deps() {
        let reqs = Requirements {
            build: vec![
                Dependency::Spec(Box::new("gcc".parse().unwrap())),
                Dependency::Spec(Box::new("make".parse().unwrap())),
            ],
            host: vec![Dependency::Spec(Box::new("python".parse().unwrap()))],
            run: vec![
                Dependency::Spec(Box::new("python".parse().unwrap())),
                Dependency::Spec(Box::new("numpy".parse().unwrap())),
            ],
            ..Default::default()
        };

        assert!(!reqs.is_empty());
        assert_eq!(reqs.build.len(), 2);
        assert_eq!(reqs.host.len(), 1);
        assert_eq!(reqs.run.len(), 2);
    }

    #[test]
    fn test_run_exports_empty() {
        let re = RunExports::new();
        assert!(re.is_empty());

        let re = RunExports {
            weak: vec![Dependency::Spec(Box::new("foo".parse().unwrap()))],
            ..Default::default()
        };
        assert!(!re.is_empty());
    }

    #[test]
    fn test_ignore_run_exports() {
        let ire = IgnoreRunExports::new();
        assert!(ire.is_empty());

        let ire = IgnoreRunExports {
            by_name: vec!["gcc".to_string()],
            ..Default::default()
        };
        assert!(!ire.is_empty());
    }

    #[test]
    fn test_pin_extraction() {
        use rattler_build_types::Pin;
        use rattler_conda_types::PackageName;

        let pin_sub = PinSubpackage {
            pin_subpackage: Pin {
                name: PackageName::try_from("mylib").unwrap(),
                args: rattler_build_types::PinArgs {
                    exact: true,
                    ..Default::default()
                },
            },
        };

        let pin_sub_no_exact = PinSubpackage {
            pin_subpackage: Pin {
                name: PackageName::try_from("otherlib").unwrap(),
                args: rattler_build_types::PinArgs {
                    exact: false,
                    ..Default::default()
                },
            },
        };

        let reqs = Requirements {
            run: vec![
                Dependency::Spec(Box::new("python".parse().unwrap())),
                Dependency::PinSubpackage(pin_sub.clone()),
                Dependency::PinSubpackage(pin_sub_no_exact.clone()),
            ],
            ..Default::default()
        };

        // Test all_pin_subpackages
        let all_pins: Vec<_> = reqs.all_pin_subpackages().collect();
        assert_eq!(all_pins.len(), 2);

        // Test exact_pin_subpackages
        let exact_pins: Vec<_> = reqs.exact_pin_subpackages().collect();
        assert_eq!(exact_pins.len(), 1);
        assert_eq!(exact_pins[0].pin_subpackage.name.as_normalized(), "mylib");
        assert!(exact_pins[0].pin_subpackage.args.exact);
    }
}
