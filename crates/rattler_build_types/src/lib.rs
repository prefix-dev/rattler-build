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

use rattler_conda_types::Subdir;

/// Returns the shared library extension for the given platform (e.g. `.so` for
/// Linux, `.dylib` for macOS, and `.dll` for Windows). Returns
/// `.not_implemented` for platforms without a known extension (e.g. `noarch`).
pub fn shlib_ext(platform: &Subdir) -> &'static str {
    if platform.is_windows() {
        ".dll"
    } else if platform.is_osx() || platform.is_ios() {
        // iOS is Mach-O like macOS.
        ".dylib"
    } else if platform.is_linux() || platform.is_android() {
        // Android is ELF like Linux (Bionic instead of glibc, same object format).
        ".so"
    } else {
        ".not_implemented"
    }
}

/// Extract the first `length` dot-separated parts of a version string and
/// concatenate them without separators. For example, `"3.11.2"` with length 2
/// gives `"311"`.
///
/// Only the version part of a match spec is considered: anything after the
/// first whitespace (e.g. a build string like `*_cpython`) is ignored, and
/// extraction stops at the first `*` wildcard. For example, `"5.44.* *_perl5"`
/// with length 3 gives `"544"`.
pub fn short_version(input: &str, length: u32) -> String {
    let version = input.split_whitespace().next().unwrap_or_default();
    let mut result = String::new();
    for part in version.split('.').take(length as usize) {
        match part.split_once('*') {
            Some((before, _)) => {
                result.push_str(before);
                break;
            }
            None => result.push_str(part),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_version_strips_wildcards_and_build_strings() {
        assert_eq!(short_version("3.11.2", 2), "311");
        assert_eq!(short_version("5.26.2", 3), "5262");
        assert_eq!(short_version("5.44", 3), "544");
        assert_eq!(short_version("5.44.*", 3), "544");
        assert_eq!(short_version("5.44.* *_perl5", 3), "544");
        assert_eq!(short_version("3.12.* *_cpython", 2), "312");
        assert_eq!(short_version("2.*", 2), "2");
        assert_eq!(short_version("2.1*", 2), "21");
        assert_eq!(short_version("", 2), "");
    }

    #[test]
    fn shlib_ext_follows_object_format() {
        // Mach-O
        assert_eq!(shlib_ext(&Subdir::Osx64), ".dylib");
        assert_eq!(shlib_ext(&Subdir::OsxArm64), ".dylib");
        assert_eq!(shlib_ext(&Subdir::IosArm64), ".dylib");
        assert_eq!(shlib_ext(&Subdir::IosSimulatorArm64), ".dylib");
        assert_eq!(shlib_ext(&Subdir::IosSimulator64), ".dylib");
        // ELF
        assert_eq!(shlib_ext(&Subdir::Linux64), ".so");
        assert_eq!(shlib_ext(&Subdir::AndroidAarch64), ".so");
        assert_eq!(shlib_ext(&Subdir::AndroidArmV7a), ".so");
        assert_eq!(shlib_ext(&Subdir::Android64), ".so");
        assert_eq!(shlib_ext(&Subdir::Android32), ".so");
        // PE
        assert_eq!(shlib_ext(&Subdir::Win64), ".dll");
        // no known extension
        assert_eq!(shlib_ext(&Subdir::NoArch), ".not_implemented");
    }
}
