use std::str::FromStr;

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

impl Pin {
    pub fn apply(&self, version: &Version, hash: &str) -> Result<MatchSpec, anyhow::Error> {
        if self.exact {
            return Ok(MatchSpec::from_str(&format!(
                "{} {} {}",
                self.name, version, hash
            ))?);
        }
        let mut spec = self.name.clone();
        let version_str = version.to_string();

        // extract same amount of digits as the pin expression (in the form of x.x.x) from version str
        if let Some(min_pin) = &self.min_pin {
            // mumber of digits in pin expression
            let pin_digits = min_pin.0.chars().filter(|c| *c == 'x').count();
            // get version strin gup the to pin_digits dot
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
            let last = pin.pop().unwrap().parse::<u64>().unwrap() + 1;
            pin.push(last.to_string());
            let pin = pin.join(".");

            if self.min_pin.is_some() {
                spec.push(',');
            } else {
                spec.push(' ');
            }
            spec.push_str(&format!("<{}", pin));
        }

        Ok(MatchSpec::from_str(&spec)?)
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
