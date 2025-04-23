//! Evaluate pin expressions and apply them to resolved dependencies
use std::{
    cmp,
    fmt::{Display, Formatter},
    str::FromStr,
};

use rattler_conda_types::{
    MatchSpec, PackageName, ParseStrictness, Version, VersionBumpError, VersionBumpType,
    VersionWithSource,
};
use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinExpression(#[serde(deserialize_with = "deserialize_pin_expression")] String);

fn deserialize_pin_expression<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    // A pin expression can only contain x and . (e.g. x.x.x)
    let s = String::deserialize(deserializer)?;
    let s = PinExpression::from_str(&s).map_err(de::Error::custom)?.0;
    Ok(s)
}

impl FromStr for PinExpression {
    type Err = std::io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.chars().any(|c| c != 'x' && c != '.') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid pin expression (can only contain x and .)",
            ));
        }
        Ok(PinExpression(s.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PinBound {
    Expression(PinExpression),
    Version(Version),
}

impl FromStr for PinBound {
    type Err = std::io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.chars().any(|c| c != 'x' && c != '.') {
            Ok(PinBound::Version(s.parse().map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, e)
            })?))
        } else {
            Ok(PinBound::Expression(s.parse()?))
        }
    }
}

impl Display for PinExpression {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A pin to a specific version of a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    /// The name of the package to pin
    pub name: PackageName,

    /// The pin arguments
    #[serde(flatten)]
    pub args: PinArgs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinArgs {
    /// A minimum pin to a version, using `x.x.x...` as syntax
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower_bound: Option<PinBound>,

    /// A pin to a version, using `x.x.x...` as syntax
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper_bound: Option<PinBound>,

    /// If an exact pin is given, we pin the exact version & hash
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

impl Default for PinArgs {
    fn default() -> Self {
        Self {
            lower_bound: Some("x.x.x.x.x.x".parse().unwrap()),
            upper_bound: Some("x".parse().unwrap()),
            exact: false,
            build: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("Could not create MatchSpec Pin: {0}")]
    MatchSpec(#[from] std::io::Error),

    #[error("Could not parse version for pinning (element not a number?): {0}")]
    CouldNotPin(String),

    #[error("lower_bound or upper_bound expression is empty string. Needs to be at least `x`")]
    EmptyPinExpression,

    #[error("Could not increment version: {0}")]
    VersionBump(#[from] VersionBumpError),

    #[error("Build specifier and exact=True are not supported together")]
    BuildSpecifierWithExact,
}

pub fn increment(version: &VersionWithSource, segments: i32) -> Result<Version, VersionBumpError> {
    if segments == 0 {
        return Err(VersionBumpError::InvalidSegment { index: 0 });
    }

    let version = version
        .clone()
        .with_segments(..cmp::min(version.segment_count(), segments as usize))
        .unwrap();

    Ok(version
        .bump(VersionBumpType::Segment(segments - 1))?
        .with_alpha()
        .remove_local()
        .into_owned())
}

impl Pin {
    /// Apply the pin to a version and hash of a resolved package. If a max_pin, min_pin or exact pin
    /// are given, the pin is applied to the version accordingly.
    pub fn apply(
        &self,
        version: &VersionWithSource,
        build_string: &str,
    ) -> Result<MatchSpec, PinError> {
        if self.args.build.is_some() && self.args.exact {
            return Err(PinError::BuildSpecifierWithExact);
        }

        if self.args.exact {
            return Ok(MatchSpec::from_str(
                &format!(
                    "{} =={} {}",
                    self.name.as_normalized(),
                    version,
                    build_string
                ),
                ParseStrictness::Strict,
            )
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?);
        }

        let mut pin_str = String::new();

        // extract same amount of digits as the pin expression (in the form of x.x.x) from version str
        match self.args.lower_bound.as_ref() {
            Some(PinBound::Expression(expression)) => {
                // number of digits in pin expression
                let pin_digits = expression.0.chars().filter(|c| *c == 'x').count();
                if pin_digits == 0 {
                    return Err(PinError::EmptyPinExpression);
                }

                // get version string up the to pin_digits dot
                let pin = version
                    .clone()
                    .with_segments(..cmp::min(pin_digits, version.segment_count()))
                    .ok_or_else(|| {
                        PinError::CouldNotPin("Failed to extract min_pin from version".to_string())
                    })?;
                pin_str.push_str(&format!(">={}", pin));
            }
            Some(PinBound::Version(version)) => {
                pin_str.push_str(&format!(">={}", version));
            }
            None => {}
        };

        match self.args.upper_bound.as_ref() {
            Some(PinBound::Expression(expression)) => {
                // number of digits in pin expression
                let pin_digits = expression.0.chars().filter(|c| *c == 'x').count();
                if pin_digits == 0 {
                    return Err(PinError::EmptyPinExpression);
                }

                // increment last digit
                let pin = increment(version, pin_digits as i32)?;

                if !pin_str.is_empty() {
                    pin_str.push(',')
                }
                pin_str.push_str(&format!("<{}", pin));
            }
            Some(PinBound::Version(version)) => {
                if !pin_str.is_empty() {
                    pin_str.push(',')
                }
                pin_str.push_str(&format!("<{}", version));
            }
            None => {}
        }

        let name = self.name.as_normalized().to_string();
        let build = self
            .args
            .build
            .as_ref()
            .map(|b| format!(" {}", b))
            .unwrap_or_default();
        Ok(MatchSpec::from_str(
            format!("{name} {pin_str}{build}").as_str().trim(),
            ParseStrictness::Strict,
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?)
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use itertools::Itertools;

    use super::*;

    #[derive(Deserialize)]
    struct TestPinExpression {
        pin: Pin,
        spec: String,
        expected: String,
    }

    #[test]
    fn test_pins() {
        // load data from the test folder
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/pins.yaml");
        let file = fs_err::File::open(p).unwrap();
        let pins: Vec<TestPinExpression> = serde_yaml::from_reader(file).unwrap();

        for test in pins {
            let spec = test.spec;
            // split the spec in 3 parts (name, version, build string)
            let (version, hash) = spec.split_whitespace().collect_tuple().unwrap();
            let version: VersionWithSource = version.parse().unwrap();
            let spec = test.pin.apply(&version, hash).unwrap();
            println!("{} -> {}", spec, test.expected);
            assert_eq!(spec.to_string(), test.expected);
        }
    }

    #[test]
    fn test_apply_pin() {
        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            args: PinArgs {
                lower_bound: Some("x.x.x".parse().unwrap()),
                upper_bound: Some("x.x.x".parse().unwrap()),
                ..Default::default()
            },
        };

        let version = VersionWithSource::from_str("1.2.3").unwrap();
        let hash = "1234567890";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3,<1.2.4.0a0");

        let short_version = VersionWithSource::from_str("1").unwrap();
        let spec = pin.apply(&short_version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1,<1.0.1.0a0");

        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            args: PinArgs {
                upper_bound: Some("x.x.x".parse().unwrap()),
                lower_bound: None,
                ..Default::default()
            },
        };

        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo <1.2.4.0a0");

        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            args: PinArgs {
                lower_bound: Some("x.x.x".parse().unwrap()),
                upper_bound: None,
                ..Default::default()
            },
        };

        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3");

        let pin = Pin {
            name: "foo".parse().unwrap(),
            args: PinArgs {
                build: Some("foo*".to_string()),
                ..Default::default()
            },
        };

        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3,<2.0a0 foo*");
    }

    #[test]
    fn test_apply_exact_pin() {
        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            args: PinArgs {
                lower_bound: Some("x.x.x".parse().unwrap()),
                upper_bound: Some("x.x.x".parse().unwrap()),
                exact: true,
                ..Default::default()
            },
        };

        let version = VersionWithSource::from_str("1.2.3").unwrap();
        let hash = "h1234_0";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo ==1.2.3 h1234_0");
    }

    #[test]
    fn test_pin_with_bounds() {
        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            args: PinArgs {
                lower_bound: Some("x.x.x".parse().unwrap()),
                upper_bound: Some("2.4".parse().unwrap()),
                ..Default::default()
            },
        };

        let version = VersionWithSource::from_str("1.2.3").unwrap();
        let hash = "h1234_0";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3,<2.4");
    }

    #[test]
    fn test_increment() {
        fn increment_to_string(input: &str, segments: i32) -> String {
            let version = VersionWithSource::from_str(input).unwrap();
            increment(&version, segments).unwrap().to_string()
        }

        assert_eq!(increment_to_string("1.2.3", 3), "1.2.4.0a0");
        assert_eq!(increment_to_string("1.2.3+4.5", 3), "1.2.4.0a0");
        assert_eq!(increment_to_string("1.2.3", 2), "1.3.0a0");
        assert_eq!(increment_to_string("1.2.3", 1), "2.0a0");
        assert_eq!(increment_to_string("1.2.3+3.4", 1), "2.0a0");
        assert_eq!(increment_to_string("9d", 1), "10a");
        assert_eq!(increment_to_string("9d", 2), "9d.1.0a0");
    }
}
