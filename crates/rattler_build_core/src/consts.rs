/// A `recipe.yaml` file might be accompanied by a `variants.yaml` file from
/// which we can read variant configuration for that specific recipe..
pub const VARIANTS_CONFIG_FILE: &str = "variants.yaml";

/// The name of the old-style configuration file (`conda_build_config.yaml`).
pub const CONDA_BUILD_CONFIG_FILE: &str = "conda_build_config.yaml";

/// This env var is set to "true" when run inside a github actions runner
pub const GITHUB_ACTIONS: &str = "GITHUB_ACTIONS";

/// This env var determines whether GitHub integration is enabled
pub const RATTLER_BUILD_ENABLE_GITHUB_INTEGRATION: &str = "RATTLER_BUILD_ENABLE_GITHUB_INTEGRATION";

/// Environment variable pointing to a file where the build script can write
/// the paths of files that should end up in the final package, one per line.
/// If the file is non-empty after the build, those paths are used as the
/// package contents instead of the default "new files in `$PREFIX`" diff.
pub const RATTLER_BUILD_PACKAGE_FILES: &str = "RATTLER_BUILD_PACKAGE_FILES";

/// Environment variable pointing to the current build step's cache declaration
/// file. A step may write input/output glob conditions to this file.
pub const RATTLER_BUILD_STEP_CACHE: &str = "RATTLER_BUILD_STEP_CACHE";

/// Directory name used under the build directory for per-step declarations and
/// their executor-managed fingerprints.
pub const STEP_CACHE_DIRECTORY_NAME: &str = ".rattler-build-step-cache";

/// File name used (under the build directory) for the package files override
/// list pointed at by [`RATTLER_BUILD_PACKAGE_FILES`]. The name is dot-prefixed
/// and namespaced so that build scripts are extremely unlikely to clobber it
/// by accident.
pub const PACKAGE_FILES_LIST_NAME: &str = ".rattler-build-package-files";
