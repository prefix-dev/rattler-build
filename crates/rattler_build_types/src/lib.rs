//! Common types shared across rattler-build crates, including glob matching,
//! version pinning, and variant configuration keys.

pub mod glob;
pub mod late_bound_path;
mod pin;
pub mod variant_config;

pub use glob::{
    AllOrGlobVec, GlobCheckerVec, GlobVec, GlobWithSource, LateBoundGlob, LateBoundGlobVec,
};
pub use late_bound_path::{LICENSE_VARS, LateBoundPath, PATCH_VARS};
pub use pin::*;
pub use variant_config::NormalizedKey;

use rattler_conda_types::Platform;

/// Returns the shared library extension for the given platform (e.g. `.so` for
/// Linux, `.dylib` for macOS, and `.dll` for Windows). Returns
/// `.not_implemented` for platforms without a known extension (e.g. `noarch`).
pub fn shlib_ext(platform: &Platform) -> &'static str {
    if platform.is_windows() {
        ".dll"
    } else if platform.is_osx() {
        ".dylib"
    } else if platform.is_linux() {
        ".so"
    } else {
        ".not_implemented"
    }
}

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
