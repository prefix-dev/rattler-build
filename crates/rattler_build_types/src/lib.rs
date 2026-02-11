pub mod glob;
mod pin;
pub mod variant_config;

pub use glob::{AllOrGlobVec, GlobCheckerVec, GlobVec, GlobWithSource};
pub use pin::*;
pub use variant_config::NormalizedKey;

/// Extract the first `length` dot-separated parts of a version string and
/// concatenate them without separators. For example, `"3.11.2"` with length 2
/// gives `"311"`.
pub fn short_version(input: &str, length: u32) -> String {
    let mut parts = input.split('.');
    let mut result = String::new();
    for _ in 0..length {
        if let Some(part) = parts.next() {
            result.push_str(part);
        }
    }
    result
}
