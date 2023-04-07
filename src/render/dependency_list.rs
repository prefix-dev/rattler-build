use std::{fmt, str::FromStr};

use super::pin::Pin;
use rattler_conda_types::MatchSpec;
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinSubpackage {
    pub pin_subpackage: Pin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compiler {
    pub compiler: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Dependency {
    #[serde(deserialize_with = "deserialize_match_spec")]
    Spec(MatchSpec),
    PinSubpackage(PinSubpackage),
    Compiler(Compiler),
}

impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DependencyVisitor;

        impl<'de> Visitor<'de> for DependencyVisitor {
            type Value = Dependency;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(
                    "a string starting with '__COMPILER', '__PIN_SUBPACKAGE', or a MatchSpec",
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Dependency, E>
            where
                E: de::Error,
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
                        .map_err(de::Error::custom)
                }
            }
        }

        deserializer.deserialize_str(DependencyVisitor)
    }
}

fn deserialize_match_spec<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(&s).map_err(de::Error::custom)
}

pub type DependencyList = Vec<Dependency>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_matchspecs() {
        let spec = r#"
- python
- python >=3.6
- python >=3.6,<3.7
- python >=3.6,<3.7.0a0
- python >=3.6,<3.7.0a0[build=py36h6de7cb9_0]
        "#
        .trim();
        let spec: DependencyList = serde_yaml::from_str(spec).unwrap();
        insta::assert_yaml_snapshot!(spec);
    }

    #[test]
    fn test_render_compiler() {
        // The following works, but doesn't play nicely with Jinja (since the jinja statements)
        // are quoted and can't produce proper YAML easily ... so we use the __COMPILER
        // syntax instead. We could / should revisit this!
        // let spec = r#"
        // - { compiler: "c" }
        // - { compiler: "cxx" }
        // - { compiler: "fortran" }
        // - { compiler: "rust" }
        //         "#
        //         .trim();
        //         let spec: DependencyList = serde_yaml::from_str(spec).unwrap();
        //         insta::assert_yaml_snapshot!(spec);
        let spec = r#"
        - __COMPILER C
        - __COMPILER CXX
        - __COMPILER FORTRAN
        - __COMPILER RUST
        "#;
        let spec: DependencyList = serde_yaml::from_str(spec).unwrap();
        insta::assert_yaml_snapshot!(spec);
    }

    #[test]
    fn test_render_pin_subpackage() {
        let pin = "- __PIN_SUBPACKAGE name MAX_PIN= MIN_PIN=x.x.x EXACT=False";
        let spec: DependencyList = serde_yaml::from_str(pin).unwrap();
        insta::assert_yaml_snapshot!(spec);

        let pin = "- __PIN_SUBPACKAGE super-package MAX_PIN=x.x MIN_PIN=x.x.x EXACT=true";
        let spec: DependencyList = serde_yaml::from_str(pin).unwrap();
        let p = &spec[0];
        let p = match p {
            Dependency::PinSubpackage(p) => p,
            _ => panic!("Expected PinSubpackage"),
        };
        assert_eq!(p.pin_subpackage.name, "super-package");
        assert_eq!(p.pin_subpackage.max_pin.as_ref().unwrap().to_string(), "x.x");
        assert_eq!(p.pin_subpackage.min_pin.as_ref().unwrap().to_string(), "x.x.x");
        assert_eq!(p.pin_subpackage.exact, true);
    }
}
