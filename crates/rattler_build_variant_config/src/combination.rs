//! Variant combination logic - computing all possible combinations of variants

use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;

use crate::error::VariantExpandError;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Internal representation of a variant key
#[derive(Debug, Clone)]
enum VariantKey {
    /// A single key with multiple possible values
    Key(NormalizedKey, Vec<Variable>),
    /// A zip key - multiple keys that are zipped together
    ZipKey(HashMap<NormalizedKey, Vec<Variable>>),
}

impl VariantKey {
    /// Get the number of variants for this key
    pub fn len(&self) -> usize {
        match self {
            VariantKey::Key(_, values) => values.len(),
            VariantKey::ZipKey(map) => map.values().next().map(|v| v.len()).unwrap_or(0),
        }
    }

    /// Get the variant at the given index
    pub fn at(&self, index: usize) -> Option<Vec<(NormalizedKey, Variable)>> {
        match self {
            VariantKey::Key(key, values) => {
                values.get(index).map(|v| vec![(key.clone(), v.clone())])
            }
            VariantKey::ZipKey(map) => {
                let mut result = Vec::new();
                for (key, values) in map {
                    if let Some(value) = values.get(index) {
                        result.push((key.clone(), value.clone()));
                    }
                }
                if result.len() == map.len() {
                    Some(result)
                } else {
                    None
                }
            }
        }
    }
}

/// Recursively compute all combinations of variants
fn find_combinations(
    variant_keys: &[VariantKey],
    index: usize,
    current: &mut Vec<(NormalizedKey, Variable)>,
    result: &mut Vec<Vec<(NormalizedKey, Variable)>>,
) {
    if index == variant_keys.len() {
        result.push(current.clone());
        return;
    }

    for i in 0..variant_keys[index].len() {
        if let Some(items) = variant_keys[index].at(i) {
            current.extend(items.clone());
            find_combinations(variant_keys, index + 1, current, result);
            for _ in 0..items.len() {
                current.pop();
            }
        }
    }
}

/// Compute all possible combinations of variants given a set of used variables
/// and zip keys.
///
/// # Arguments
///
/// * `variants` - All available variants (key -> list of values)
/// * `zip_keys` - Keys that should be zipped together
/// * `used_vars` - Variables that are actually used (only these will be in the result)
/// * `already_used_vars` - Optional filter to only return combinations matching these values
///
/// # Returns
///
/// A vector of maps, where each map represents one variant combination
pub fn compute_combinations(
    variants: &BTreeMap<NormalizedKey, Vec<Variable>>,
    zip_keys: &[Vec<NormalizedKey>],
    used_vars: &HashSet<NormalizedKey>,
    already_used_vars: Option<&BTreeMap<NormalizedKey, Variable>>,
) -> Result<Vec<BTreeMap<NormalizedKey, Variable>>, VariantExpandError> {
    // Validate zip keys
    validate_zip_keys(variants, zip_keys)?;

    // Build zip keys that are actually used
    let used_zip_keys = zip_keys
        .iter()
        .filter(|zip| zip.iter().any(|key| used_vars.contains(key)))
        .map(|zip| {
            let mut map = HashMap::new();
            for key in zip {
                if !used_vars.contains(key) {
                    continue;
                }
                if let Some(values) = variants.get(key) {
                    map.insert(key.clone(), values.clone());
                }
            }
            VariantKey::ZipKey(map)
        })
        .collect::<Vec<_>>();

    // Build individual variant keys (not part of any zip)
    let variant_keys = used_vars
        .iter()
        .filter_map(|key| {
            if let Some(values) = variants.get(key) {
                if !zip_keys.iter().any(|zip| zip.contains(key)) {
                    return Some(VariantKey::Key(key.clone(), values.clone()));
                }
            }
            None
        })
        .collect::<Vec<_>>();

    // Combine zip keys and individual keys
    let all_keys = used_zip_keys
        .into_iter()
        .chain(variant_keys)
        .collect::<Vec<_>>();

    // Compute all combinations
    let mut combinations = Vec::new();
    let mut current = Vec::new();
    find_combinations(&all_keys, 0, &mut current, &mut combinations);

    // Convert to BTreeMaps and sort for deterministic output
    let mut result: Vec<_> = combinations
        .iter()
        .map(|combination| {
            combination
                .iter()
                .cloned()
                .collect::<BTreeMap<NormalizedKey, Variable>>()
        })
        .collect();

    // Sort combinations by their serialized form for deterministic output
    result.sort_by_cached_key(|combo| format!("{:?}", combo));

    // Filter by already_used_vars if provided
    if let Some(already_used_vars) = already_used_vars {
        let result = result
            .into_iter()
            .filter(|combination| {
                if already_used_vars.is_empty() {
                    true
                } else {
                    already_used_vars
                        .iter()
                        .all(|(key, value)| combination.get(key) == Some(value))
                }
            })
            .collect();
        Ok(result)
    } else {
        Ok(result)
    }
}

/// Validate that zip keys are properly structured and have matching lengths
fn validate_zip_keys(
    variants: &BTreeMap<NormalizedKey, Vec<Variable>>,
    zip_keys: &[Vec<NormalizedKey>],
) -> Result<(), VariantExpandError> {
    for zip in zip_keys {
        if zip.len() < 2 {
            return Err(VariantExpandError::InvalidZipKeyStructure);
        }

        let mut prev_len = None;
        for key in zip {
            let value = match variants.get(key) {
                None => {
                    return Err(VariantExpandError::MissingVariantKey(key.normalize()));
                }
                Some(value) => value,
            };

            if let Some(l) = prev_len {
                if l != value.len() {
                    return Err(VariantExpandError::InvalidZipKeyLength(key.normalize()));
                }
            }
            prev_len = Some(value.len());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_combinations() {
        let mut variants = BTreeMap::new();
        variants.insert("python".into(), vec!["3.9".into(), "3.10".into()]);
        variants.insert("numpy".into(), vec!["1.20".into(), "1.21".into()]);

        let mut used_vars = HashSet::new();
        used_vars.insert("python".into());
        used_vars.insert("numpy".into());

        let result = compute_combinations(&variants, &[], &used_vars, None).unwrap();

        // Should create 2x2 = 4 combinations
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_zip_keys() {
        let mut variants = BTreeMap::new();
        variants.insert("python".into(), vec!["3.9".into(), "3.10".into()]);
        variants.insert("numpy".into(), vec!["1.20".into(), "1.21".into()]);

        let zip_keys = vec![vec!["python".into(), "numpy".into()]];

        let mut used_vars = HashSet::new();
        used_vars.insert("python".into());
        used_vars.insert("numpy".into());

        let result = compute_combinations(&variants, &zip_keys, &used_vars, None).unwrap();

        // Should create 2 combinations (zipped)
        assert_eq!(result.len(), 2);

        // Check that they're properly zipped
        assert_eq!(result[0].get(&"python".into()).unwrap().to_string(), "3.9");
        assert_eq!(result[0].get(&"numpy".into()).unwrap().to_string(), "1.20");
        assert_eq!(result[1].get(&"python".into()).unwrap().to_string(), "3.10");
        assert_eq!(result[1].get(&"numpy".into()).unwrap().to_string(), "1.21");
    }

    #[test]
    fn test_invalid_zip_length() {
        let mut variants = BTreeMap::new();
        variants.insert("python".into(), vec!["3.9".into(), "3.10".into()]);
        variants.insert("numpy".into(), vec!["1.20".into()]);

        let zip_keys = vec![vec!["python".into(), "numpy".into()]];

        let result = validate_zip_keys(&variants, &zip_keys);
        assert!(result.is_err());
    }
}
