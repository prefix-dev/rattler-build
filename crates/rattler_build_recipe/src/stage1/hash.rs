//! Hash computation for build strings
//!
//! This module computes a hash value from the actual variant (subset of variant variables
//! that were actually used during recipe evaluation). The hash is used in the build string
//! to uniquely identify package builds with different configurations.

use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;
use std::collections::BTreeMap;

/// Compute a hash string from a variant map with version prefix
///
/// The hash is computed by:
/// 1. Computing the version prefix (e.g., "py312", "np120") based on special packages
/// 2. Serializing the variant to JSON with sorted keys (BTreeMap ensures this)
/// 3. Computing SHA1 hash of the JSON bytes
/// 4. Taking the first 7 characters of the hex digest
/// 5. Combining as: {prefix}h{hash} (e.g., "py312h507f6e9")
///
/// # Arguments
///
/// * `variant` - Map of variant variables that were actually used
/// * `noarch` - NoArch type of the package
///
/// # Returns
///
/// A hash string with prefix (e.g., "py312h507f6e9" or just "h507f6e9" if no prefix)
pub fn compute_hash(
    variant: &BTreeMap<NormalizedKey, Variable>,
    noarch: &rattler_conda_types::NoArchType,
) -> String {
    use sha1::{Digest, Sha1};

    // Compute the version prefix (py, np, etc.)
    let prefix = compute_hash_prefix(variant, noarch);

    // Serialize variant to JSON with Python-compatible formatting
    let json_bytes = serialize_variant_to_json(variant);

    // Compute SHA1 hash
    let mut hasher = Sha1::new();
    hasher.update(&json_bytes);
    let result = hasher.finalize();
    let hex = format!("{:x}", result);

    // Take first 7 characters and combine with prefix
    let hash = hex.chars().take(7).collect::<String>();
    format!("{}h{}", prefix, hash)
}

/// Compute the hash prefix based on special packages in the variant
///
/// For example:
/// - python 3.12 → "py312"
/// - numpy 1.20 → "np120"
/// - Combined: "py312np120"
///
/// Order of prefixes: np, py, pl, lua, r
fn compute_hash_prefix(
    variant: &BTreeMap<NormalizedKey, Variable>,
    noarch: &rattler_conda_types::NoArchType,
) -> String {
    if noarch.is_python() {
        return "py".to_string();
    }

    let mut map = std::collections::HashMap::new();

    for (variant_key, version_spec) in variant.iter() {
        let key_str = variant_key.0.as_str();
        let prefix = match key_str {
            "numpy" => "np",
            "python" => "py",
            "perl" => "pl",
            "lua" => "lua",
            "r" | "r-base" | "r_base" => "r",
            _ => continue,
        };

        let version_length = match prefix {
            "pl" => 3, // perl uses 3 digits (e.g., pl526)
            _ => 2,    // others use 2 (e.g., py312, np120)
        };

        map.insert(
            prefix.to_string(),
            short_version_from_spec(&version_spec.to_string(), version_length),
        );
    }

    // Order matters: np, py, pl, lua, r
    let order = vec!["np", "py", "pl", "lua", "r"];
    let mut result = String::new();
    for key in order {
        if let Some(value) = map.get(key) {
            result.push_str(&format!("{}{}", key, value));
        }
    }

    result
}

/// Extract short version from a version spec (e.g., "3.12.1" → "312")
fn short_version_from_spec(input: &str, length: u32) -> String {
    let mut parts = input.split('.');
    let mut result = String::new();
    for _ in 0..length {
        if let Some(part) = parts.next() {
            result.push_str(part);
        }
    }
    result
}

/// Serialize a variant map to JSON bytes using Python-compatible formatting
///
/// This uses a custom formatter to match Python's JSON output for compatibility
/// with existing hash computations.
fn serialize_variant_to_json(variant: &BTreeMap<NormalizedKey, Variable>) -> Vec<u8> {
    // Convert to a format suitable for JSON serialization
    let json_map: BTreeMap<String, serde_json::Value> = variant
        .iter()
        .map(|(k, v)| {
            let value = variable_to_json_value(v);
            (k.0.clone(), value)
        })
        .collect();

    // Serialize with custom formatting to match Python output
    let mut buf = Vec::new();
    let formatter = PythonJsonFormatter::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
    serde::Serialize::serialize(&json_map, &mut serializer).expect("Failed to serialize variant");

    buf
}

/// Convert a Variable to a serde_json::Value
fn variable_to_json_value(var: &Variable) -> serde_json::Value {
    // Variable is an opaque type, so we serialize it through serde
    serde_json::to_value(var).unwrap_or(serde_json::Value::Null)
}

/// Custom JSON formatter that matches Python's output format
///
/// This ensures compatibility with existing hash computations by using
/// ", " (comma-space) between elements instead of ",\n".
struct PythonJsonFormatter {
    current_indent: usize,
}

impl PythonJsonFormatter {
    fn new() -> Self {
        Self { current_indent: 0 }
    }
}

impl serde_json::ser::Formatter for PythonJsonFormatter {
    fn begin_array<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        self.current_indent += 1;
        writer.write_all(b"[")
    }

    fn end_array<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        self.current_indent -= 1;
        writer.write_all(b"]")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn end_array_value<W>(&mut self, _writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        Ok(())
    }

    fn begin_object<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        self.current_indent += 1;
        writer.write_all(b"{")
    }

    fn end_object<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        self.current_indent -= 1;
        writer.write_all(b"}")
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn end_object_key<W>(&mut self, _writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        Ok(())
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }

    fn end_object_value<W>(&mut self, _writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_hash_simple() {
        use rattler_conda_types::NoArchType;

        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("python"),
            Variable::from("3.11".to_string()),
        );

        let hash = compute_hash(&variant, &NoArchType::none());

        // Hash should include prefix "py311h" + 7 hex chars = "py311h" + hash
        assert!(hash.starts_with("py311h"));
        assert!(hash.len() > 7);
        // The hex part should be valid hex
        assert!(hash[6..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_compute_hash_deterministic() {
        use rattler_conda_types::NoArchType;

        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("python"),
            Variable::from("3.11".to_string()),
        );
        variant.insert(
            NormalizedKey::from("numpy"),
            Variable::from("1.20".to_string()),
        );

        let hash1 = compute_hash(&variant, &NoArchType::none());
        let hash2 = compute_hash(&variant, &NoArchType::none());

        // Same input should produce same hash
        assert_eq!(hash1, hash2);
        // Should have both numpy and python prefix
        assert!(hash1.contains("np") && hash1.contains("py"));
    }

    #[test]
    fn test_compute_hash_different_variants() {
        use rattler_conda_types::NoArchType;

        let mut variant1 = BTreeMap::new();
        variant1.insert(
            NormalizedKey::from("python"),
            Variable::from("3.11".to_string()),
        );

        let mut variant2 = BTreeMap::new();
        variant2.insert(
            NormalizedKey::from("python"),
            Variable::from("3.12".to_string()),
        );

        let hash1 = compute_hash(&variant1, &NoArchType::none());
        let hash2 = compute_hash(&variant2, &NoArchType::none());

        // Different inputs should produce different hashes
        assert_ne!(hash1, hash2);
        // Should have different python versions in prefix
        assert!(hash1.starts_with("py311"));
        assert!(hash2.starts_with("py312"));
    }
}
