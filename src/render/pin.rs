//! Evaluate pin expressions and apply them to resolved dependencies
use std::{
    cmp,
    fmt::{Display, Formatter},
    str::FromStr,
};

use rattler_conda_types::{
    MatchSpec, PackageName, ParseStrictness, Version, VersionBumpError, VersionBumpType,
};
use serde::{de, Deserialize, Deserializer, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PinArgs {
    /// A pin to a version, using `x.x.x...` as syntax
    #[serde(default)]
    pub max_pin: Option<PinExpression>,

    /// A minimum pin to a version, using `x.x.x...` as syntax
    #[serde(default)]
    pub min_pin: Option<PinExpression>,

    /// A lower bound to a version, using a regular version string
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower_bound: Option<String>,

    /// A upper bound to a version, using a regular version string
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper_bound: Option<String>,

    /// If an exact pin is given, we pin the exact version & hash
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("Could not create MatchSpec Pin: {0}")]
    MatchSpec(#[from] std::io::Error),

    #[error("Could not parse version for pinning (element not a number?): {0}")]
    CouldNotPin(String),

    #[error("max_pin or min_pin expression is empty string. Needs to be at least `x`")]
    EmptyPinExpression,

    #[error("Could not increment version: {0}")]
    VersionBump(#[from] VersionBumpError),
}

pub fn increment(version: &Version, segments: i32) -> Result<Version, VersionBumpError> {
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
        .into_owned())
}

impl Pin {
    /// Apply the pin to a version and hash of a resolved package. If a max_pin, min_pin or exact pin
    /// are given, the pin is applied to the version accordingly.
    pub fn apply(&self, version: &Version, hash: &str) -> Result<MatchSpec, PinError> {
        if self.args.exact {
            return Ok(MatchSpec::from_str(
                &format!("{} {} {}", self.name.as_normalized(), version, hash),
                ParseStrictness::Strict,
            )
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?);
        }

        let mut pin_str = String::new();

        // extract same amount of digits as the pin expression (in the form of x.x.x) from version str
        if let Some(lower_bound) = &self.args.lower_bound {
            pin_str.push_str(&format!(">={}", lower_bound));
        } else if let Some(min_pin) = &self.args.min_pin {
            // number of digits in pin expression
            let pin_digits = min_pin.0.chars().filter(|c| *c == 'x').count();
            if pin_digits == 0 {
                return Err(PinError::EmptyPinExpression);
            }

            // get version string up the to pin_digits dot
            let pin = version
                .clone()
                .with_segments(..cmp::min(pin_digits, version.segment_count()))
                .ok_or_else(|| {
                    PinError::CouldNotPin(format!("Failed to extract min_pin from version"))
                })?;
            pin_str.push_str(&format!(">={}", pin));
        };

        if let Some(upper_bound) = &self.args.upper_bound {
            if !pin_str.is_empty() {
                pin_str.push(',')
            }
            pin_str.push_str(&format!("<{}", upper_bound));
        } else if let Some(max_pin) = &self.args.max_pin {
            // number of digits in pin expression
            let pin_digits = max_pin.0.chars().filter(|c| *c == 'x').count();
            if pin_digits == 0 {
                return Err(PinError::EmptyPinExpression);
            }

            // increment last digit
            let pin = increment(&version, pin_digits as i32)?;

            if !pin_str.is_empty() {
                pin_str.push(',')
            }
            pin_str.push_str(&format!("<{}", pin));
        }

        let name = self.name.as_normalized().to_string();
        Ok(MatchSpec::from_str(
            format!("{name} {pin_str}").as_str().trim(),
            ParseStrictness::Strict,
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?)
    }

    pub(crate) fn internal_repr(&self) -> String {
        let max_pin_str = if let Some(max_pin) = &self.max_pin {
            format!("{}", max_pin)
        } else {
            "".to_string()
        };

        let min_pin_str = if let Some(min_pin) = &self.min_pin {
            format!("{}", min_pin)
        } else {
            "".to_string()
        };

        format!(
            "{} MAX_PIN={} MIN_PIN={} EXACT={}",
            self.name.as_normalized(),
            max_pin_str,
            min_pin_str,
            self.exact
        )
    }

    pub(crate) fn from_internal_repr(s: &str) -> Self {
        let parts = s.split(' ').collect::<Vec<_>>();
        let name = parts[0].to_string();
        let max_pin = parts[1];
        let min_pin = parts[2];
        let exact = parts[3];

        let max_pin = if max_pin == "MAX_PIN=" {
            None
        } else {
            let max_pin = max_pin
                .strip_prefix("MAX_PIN=")
                .expect("Could not parse max pin: invalid prefix");
            Some(PinExpression::from_str(max_pin).expect("Could not parse max pin"))
        };

        let min_pin = if min_pin == "MIN_PIN=" {
            None
        } else {
            let min_pin = min_pin
                .strip_prefix("MIN_PIN=")
                .expect("Could not parse min pin: invalid prefix");
            Some(PinExpression::from_str(min_pin).expect("Could not parse min pin"))
        };

        let exact = exact == "EXACT=true";
        let package_name = PackageName::try_from(name)
            .expect("could not parse back package name from internal representation");
        Pin {
            name: package_name,
            max_pin,
            min_pin,
            exact,
        }
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
            let version: Version = version.parse().unwrap();
            let spec = test.pin.apply(&version, hash).unwrap();
            println!("{} -> {}", spec.to_string(), test.expected);
            assert_eq!(spec.to_string(), test.expected);
        }
    }

    #[test]
    fn test_apply_pin() {
        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            max_pin: Some(PinExpression("x.x.x".to_string())),
            min_pin: Some(PinExpression("x.x.x".to_string())),
            exact: false,
        };

        let version = Version::from_str("1.2.3").unwrap();
        let hash = "1234567890";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3,<1.2.4.0a0");

        let short_version = Version::from_str("1").unwrap();
        let spec = pin.apply(&short_version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1,<1.0.1.0a0");

        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            max_pin: Some(PinExpression("x.x.x".to_string())),
            min_pin: None,
            exact: false,
        };

        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo <1.2.4.0a0");

        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            max_pin: None,
            min_pin: Some(PinExpression("x.x.x".to_string())),
            exact: false,
        };

        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3");
    }

    #[test]
    fn test_apply_exact_pin() {
        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            max_pin: Some(PinExpression("x.x.x".to_string())),
            min_pin: Some(PinExpression("x.x.x".to_string())),
            exact: true,
        };

        let version = Version::from_str("1.2.3").unwrap();
        let hash = "h1234_0";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo ==1.2.3 h1234_0");
    }

    #[test]
    fn test_pin_with_bounds() {
        let pin = Pin {
            name: PackageName::from_str("foo").unwrap(),
            min_pin: Some(PinExpression("x.x.x".to_string())),
            max_pin: None,
            lower_bound: None,
            upper_bound: Some("2.4".to_string()),
            exact: false,
        };

        let version = Version::from_str("1.2.3").unwrap();
        let hash = "h1234_0";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3,<2.4");
    }

    #[test]
    fn test_increment() {
        fn increment_to_string(input: &str, segments: i32) -> String {
            let version = Version::from_str(input).unwrap();
            increment(&version, segments).unwrap().to_string()
        }

        assert_eq!(increment_to_string("1.2.3", 3), "1.2.4.0a0");
        assert_eq!(increment_to_string("1.2.3", 2), "1.3.0a0");
        assert_eq!(increment_to_string("1.2.3", 1), "2.0a0");
        assert_eq!(increment_to_string("9d", 1), "10a");
        assert_eq!(increment_to_string("9d", 2), "9d.1.0a0");
    }
}
