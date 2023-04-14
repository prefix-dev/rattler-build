use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use rattler_conda_types::{MatchSpec, Version};
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
    pub name: String,

    /// A pin to a version, using `x.x.x...` as syntax
    pub max_pin: Option<PinExpression>,

    /// A minimum pin to a version, using `x.x.x...` as syntax
    pub min_pin: Option<PinExpression>,

    /// If an exact pin is given, we pin the exact version & hash
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("Could not create MatchSpec Pin: {0}")]
    MatchSpec(#[from] std::io::Error),

    #[error("Could not parse version for pinning (element not a number?) {0}")]
    CouldNotPin(String),

    #[error("max_pin or min_pin expression is empty string. Needs to be at least `x`")]
    EmptyPinExpression,
}

impl Pin {
    /// Apply the pin to a version and hash of a resolved package. If a max_pin, min_pin or exact pin
    /// are given, the pin is applied to the version accordingly.
    pub fn apply(&self, version: &Version, hash: &str) -> Result<MatchSpec, PinError> {
        if self.exact {
            return Ok(
                MatchSpec::from_str(&format!("{} {} {}", self.name, version, hash))
                    // TODO use MatchSpecError when it becomes accessible
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            );
        }
        let mut spec = self.name.clone();
        let version_str = version.to_string();

        // extract same amount of digits as the pin expression (in the form of x.x.x) from version str
        if let Some(min_pin) = &self.min_pin {
            // mumber of digits in pin expression
            let pin_digits = min_pin.0.chars().filter(|c| *c == 'x').count();
            if pin_digits == 0 {
                return Err(PinError::EmptyPinExpression);
            }

            // get version string up the to pin_digits dot
            let pin = version_str
                .split('.')
                .take(pin_digits)
                .collect::<Vec<_>>()
                .join(".");
            spec.push_str(&format!(" >={}", pin));
        }

        if let Some(max_pin) = &self.max_pin {
            // mumber of digits in pin expression
            let pin_digits = max_pin.0.chars().filter(|c| *c == 'x').count();
            if pin_digits == 0 {
                return Err(PinError::EmptyPinExpression);
            }
            // get version strin gup the to pin_digits dot
            let mut pin = version_str
                .split('.')
                .take(pin_digits)
                .map(|s| s.to_string())
                .collect::<Vec<_>>();

            // fill up with 0s
            while pin.len() < pin_digits {
                pin.push("0".to_string());
            }

            // increment last digit
            let last = pin
                .pop()
                .unwrap_or("0".to_string())
                .parse::<u64>()
                .map_err(|_| PinError::CouldNotPin(version_str.clone()))?
                + 1;
            pin.push(last.to_string());
            let pin = pin.join(".");

            if self.min_pin.is_some() {
                spec.push(',');
            } else {
                spec.push(' ');
            }
            spec.push_str(&format!("<{}", pin));
        }

        Ok(MatchSpec::from_str(&spec)
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
            self.name, max_pin_str, min_pin_str, self.exact
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
            Some(PinExpression::from_str(&max_pin[8..]).expect("Could not parse max pin"))
        };

        let min_pin = if min_pin == "MIN_PIN=" {
            None
        } else {
            Some(PinExpression::from_str(&min_pin[8..]).expect("Could not parse min pin"))
        };

        let exact = exact == "EXACT=true";

        Pin {
            name,
            max_pin,
            min_pin,
            exact,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_apply_pin() {
        let pin = Pin {
            name: "foo".to_string(),
            max_pin: Some(PinExpression("x.x.x".to_string())),
            min_pin: Some(PinExpression("x.x.x".to_string())),
            exact: false,
        };

        let version = Version::from_str("1.2.3").unwrap();
        let hash = "1234567890";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1.2.3,<1.2.4");

        let short_version = Version::from_str("1").unwrap();
        let spec = pin.apply(&short_version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo >=1,<1.0.1");

        let pin = Pin {
            name: "foo".to_string(),
            max_pin: Some(PinExpression("x.x.x".to_string())),
            min_pin: None,
            exact: false,
        };

        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo <1.2.4");

        let pin = Pin {
            name: "foo".to_string(),
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
            name: "foo".to_string(),
            max_pin: Some(PinExpression("x.x.x".to_string())),
            min_pin: Some(PinExpression("x.x.x".to_string())),
            exact: true,
        };

        let version = Version::from_str("1.2.3").unwrap();
        let hash = "h1234_0";
        let spec = pin.apply(&version, hash).unwrap();
        assert_eq!(spec.to_string(), "foo ==1.2.3 h1234_0");
    }
}
