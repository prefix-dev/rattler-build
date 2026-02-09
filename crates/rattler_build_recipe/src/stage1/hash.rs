//! Compute the build string / hash info for a given variant
use std::collections::{BTreeMap, HashMap};

use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::NoArchType;
use serde::{Deserialize, Serialize};
use serde_json::ser::Formatter;
use sha1::{Digest, Sha1};

/// A hash will be added if all of these are true for any dependency:
///
/// 1. package is an explicit dependency in build, host, or run deps
/// 2. package has a matching entry in conda_build_config.yaml which is a pin to a specific
///    version, not a lower bound
/// 3. that package is not ignored by ignore_version (not implemented yet)
///
/// The hash is computed based on the pinning value, NOT the build
///    dependency build string. This means hashes won't change as often,
///    but it also means that if run_exports is overly permissive,
///    software may break more often.
///
/// A hash will also ALWAYS be added when a compiler package is a build
///    or host dependency. Reasoning for that is that the compiler
///    package represents compiler flags and other things that can and do
///    dramatically change compatibility. It is much more risky to drop
///    this info (by dropping the hash) than it is for other software.
///
/// used variables - anything with a value in conda_build_config.yaml that applies to this
///    recipe.  Includes compiler if compiler jinja2 function is used.
///
/// This implements a formatter that uses the same formatting as
/// as the standard lib python `json.dumps()`
#[derive(Clone, Debug)]
struct PythonFormatter {}

impl Formatter for PythonFormatter {
    #[inline]
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

    #[inline]
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

    #[inline]
    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }
}

use rattler_build_types::short_version;

/// The hash info for a given variant
#[derive(Debug, PartialEq, Clone, Eq, Hash, Serialize, Deserialize)]
pub struct HashInfo {
    /// The hash (first 7 letters of the sha1sum)
    pub hash: String,

    /// The hash prefix (e.g. `py38` or `np111`)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
}

/// Represents the input to compute the hash
#[derive(Debug, Clone)]
pub struct HashInput(String);

impl HashInput {
    /// Create a new hash input from a variant
    pub fn from_variant(variant: &BTreeMap<NormalizedKey, Variable>) -> Self {
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, PythonFormatter {});

        // BTree has sorted keys, which is important for hashing
        variant
            .serialize(&mut ser)
            .expect("Failed to serialize input");

        Self(String::from_utf8(buf).expect("Failed to convert to string"))
    }

    /// Get the hash input as a string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the hash input as bytes
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Display for HashInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}h{}", self.prefix, self.hash)
    }
}

impl HashInfo {
    fn hash_prefix(variant: &BTreeMap<NormalizedKey, Variable>, noarch: &NoArchType) -> String {
        if noarch.is_python() {
            return "py".to_string();
        }

        let mut map: HashMap<String, String> = HashMap::new();

        for (variant_key, version_spec) in variant.iter() {
            let prefix = match variant_key.normalize().as_str() {
                "numpy" => "np",
                "python" => "py",
                "perl" => "pl",
                "lua" => "lua",
                "r" | "r-base" | "r_base" => "r",
                _ => continue,
            };

            let version_length = match prefix {
                "pl" => 3,
                _ => 2,
            };

            map.insert(
                prefix.to_string(),
                short_version(&version_spec.to_string(), version_length),
            );
        }

        let order = vec!["np", "py", "pl", "lua", "r", "mro"];
        let mut result = String::new();
        for key in order {
            if let Some(value) = map.get(key) {
                result.push_str(format!("{}{}", key, value).as_str());
            }
        }
        result
    }

    fn hash_from_input(hash_input: &HashInput) -> String {
        let mut hasher = Sha1::new();
        hasher.update(hash_input.as_bytes());
        let result = hasher.finalize();

        const HASH_LENGTH: usize = 7;

        let res = format!("{:x}", result);
        res[..HASH_LENGTH].to_string()
    }

    /// Compute the build string for a given variant
    pub fn from_variant(variant: &BTreeMap<NormalizedKey, Variable>, noarch: &NoArchType) -> Self {
        Self {
            hash: Self::hash_from_input(&HashInput::from_variant(variant)),
            prefix: Self::hash_prefix(variant, noarch),
        }
    }
}

/// Compute a hash string from a variant map with version prefix
///
/// This is a convenience function that returns a tuple of (prefix, hash)
/// for compatibility with existing code.
pub fn compute_hash(
    variant: &BTreeMap<NormalizedKey, Variable>,
    noarch: &NoArchType,
) -> (String, String) {
    let hash_info = HashInfo::from_variant(variant, noarch);
    (hash_info.prefix, hash_info.hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_hash_full_variant() {
        let mut input = BTreeMap::new();
        input.insert("rust_compiler".into(), "rust".into());
        input.insert("build_platform".into(), "osx-64".into());
        input.insert("c_compiler".into(), "clang".into());
        input.insert("target_platform".into(), "osx-arm64".into());
        input.insert("openssl".into(), "3".into());
        input.insert(
            "CONDA_BUILD_SYSROOT".into(),
            "/Applications/Xcode_13.2.1.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX11.0.sdk".into()
        );
        input.insert("channel_targets".into(), "conda-forge main".into());
        input.insert("python".into(), "3.11.* *_cpython".into());
        input.insert("c_compiler_version".into(), "14".into());

        let hash_info = HashInfo::from_variant(&input, &NoArchType::none());
        assert_eq!(hash_info.to_string(), "py311h507f6e9");
        assert_eq!(hash_info.prefix, "py311");
        assert_eq!(hash_info.hash, "507f6e9");
    }

    #[test]
    fn test_compute_hash_simple() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("python"),
            Variable::from("3.11".to_string()),
        );

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Hash should include prefix "py311" + 7 hex chars
        assert_eq!(hash_info.prefix, "py311");
        assert_eq!(hash_info.hash.len(), 7);
        // The hex part should be valid hex
        assert!(hash_info.hash.chars().all(|c| c.is_ascii_hexdigit()));
        // Should match display format
        assert_eq!(hash_info.to_string(), format!("py311h{}", hash_info.hash));
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("python"),
            Variable::from("3.11".to_string()),
        );
        variant.insert(
            NormalizedKey::from("numpy"),
            Variable::from("1.20".to_string()),
        );

        let hash1 = HashInfo::from_variant(&variant, &NoArchType::none());
        let hash2 = HashInfo::from_variant(&variant, &NoArchType::none());

        // Same input should produce same hash
        assert_eq!(hash1, hash2);
        // Should have both numpy and python prefix (in correct order: np, py)
        assert!(hash1.prefix.contains("np") && hash1.prefix.contains("py"));
        assert_eq!(hash1.prefix, "np120py311");
    }

    #[test]
    fn test_compute_hash_different_variants() {
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

        let hash1 = HashInfo::from_variant(&variant1, &NoArchType::none());
        let hash2 = HashInfo::from_variant(&variant2, &NoArchType::none());

        // Different inputs should produce different hashes
        assert_ne!(hash1, hash2);
        // Should have different python versions in prefix
        assert_eq!(hash1.prefix, "py311");
        assert_eq!(hash2.prefix, "py312");
        // The hash parts should also differ
        assert_ne!(hash1.hash, hash2.hash);
    }

    #[test]
    fn test_compute_hash_compatibility() {
        let mut variant = BTreeMap::new();
        variant.insert("python".into(), "3.11".into());
        variant.insert("numpy".into(), "1.20".into());

        let (prefix, hash) = compute_hash(&variant, &NoArchType::none());
        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // The compute_hash function should produce the same results as HashInfo
        assert_eq!(prefix, hash_info.prefix);
        assert_eq!(hash, hash_info.hash);
    }

    #[test]
    fn test_hash_prefix_ordering() {
        let mut variant = BTreeMap::new();
        variant.insert(NormalizedKey::from("python"), Variable::from("3.10"));
        variant.insert(NormalizedKey::from("numpy"), Variable::from("1.21"));
        variant.insert(NormalizedKey::from("lua"), Variable::from("5.4"));
        variant.insert(NormalizedKey::from("r"), Variable::from("4.2"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Check that prefixes are in the correct order: np, py, pl, lua, r
        assert_eq!(hash_info.prefix, "np121py310lua54r42");
    }

    #[test]
    fn test_hash_prefix_perl() {
        let mut variant = BTreeMap::new();
        variant.insert(NormalizedKey::from("perl"), Variable::from("5.26.2"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Perl uses 3 digits instead of 2
        // The function extracts the first 3 parts: "5", "26", "2" -> "5262"
        assert_eq!(hash_info.prefix, "pl5262");

        // Test with a simpler version
        let mut variant2 = BTreeMap::new();
        variant2.insert(NormalizedKey::from("perl"), Variable::from("5.26"));
        let hash_info2 = HashInfo::from_variant(&variant2, &NoArchType::none());
        assert_eq!(hash_info2.prefix, "pl526");
    }

    #[test]
    fn test_hash_noarch_python() {
        let mut variant = BTreeMap::new();
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11"));
        variant.insert(NormalizedKey::from("numpy"), Variable::from("1.20"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::python());

        // For noarch python, prefix should just be "py" (without version)
        assert_eq!(hash_info.prefix, "py");
    }

    #[test]
    fn test_hash_r_variants() {
        // Test that all R variant keys produce the same prefix
        let variants = vec![("r", "4.1"), ("r-base", "4.1"), ("r_base", "4.1")];

        for (key, version) in variants {
            let mut variant = BTreeMap::new();
            variant.insert(NormalizedKey::from(key), Variable::from(version));
            let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());
            // All should have the same prefix
            assert_eq!(hash_info.prefix, "r41", "Failed for key: {}", key);
        }

        // However, the full hash will differ because NormalizedKey normalizes differently
        // (e.g., "r-base" becomes "r_base"), so the JSON representation differs
        let mut variant1 = BTreeMap::new();
        variant1.insert(NormalizedKey::from("r"), Variable::from("4.1"));
        let hash1 = HashInfo::from_variant(&variant1, &NoArchType::none());

        let mut variant2 = BTreeMap::new();
        variant2.insert(NormalizedKey::from("r-base"), Variable::from("4.1"));
        let hash2 = HashInfo::from_variant(&variant2, &NoArchType::none());

        // Prefixes should be the same
        assert_eq!(hash1.prefix, hash2.prefix);
        // But hashes might differ due to key normalization in the JSON
    }

    #[test]
    fn test_hash_input_json_format() {
        let mut variant = BTreeMap::new();
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11"));
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );

        let hash_input = HashInput::from_variant(&variant);
        let json_str = hash_input.as_str();

        // Should be valid JSON
        assert!(json_str.starts_with('{'));
        assert!(json_str.ends_with('}'));
        // Should contain both keys (BTreeMap ensures sorted order)
        assert!(json_str.contains("python"));
        assert!(json_str.contains("target_platform"));
        // Should use Python-style formatting with ", " separators
        assert!(json_str.contains(": "));
    }

    #[test]
    fn test_hash_empty_variant() {
        let variant = BTreeMap::new();
        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Empty variant should have empty prefix but still produce a hash
        assert_eq!(hash_info.prefix, "");
        assert_eq!(hash_info.hash.len(), 7);
        // Display should just be "h" + hash
        assert_eq!(hash_info.to_string(), format!("h{}", hash_info.hash));
    }

    #[test]
    fn test_hash_only_non_prefix_vars() {
        let mut variant = BTreeMap::new();
        variant.insert(NormalizedKey::from("openssl"), Variable::from("3"));
        variant.insert(NormalizedKey::from("c_compiler"), Variable::from("gcc"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // No special prefix variables, so prefix should be empty
        assert_eq!(hash_info.prefix, "");
        // But should still have a hash
        assert_eq!(hash_info.hash.len(), 7);
    }

    #[test]
    fn test_short_version_extraction() {
        let mut variant = BTreeMap::new();
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11.5"));
        variant.insert(NormalizedKey::from("numpy"), Variable::from("1.20.3"));
        variant.insert(NormalizedKey::from("perl"), Variable::from("5.26.2"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Should extract short versions correctly
        assert!(hash_info.prefix.contains("py311")); // python: 2 digits
        assert!(hash_info.prefix.contains("np120")); // numpy: 2 digits
        assert!(hash_info.prefix.contains("pl526")); // perl: 3 digits
    }
}
