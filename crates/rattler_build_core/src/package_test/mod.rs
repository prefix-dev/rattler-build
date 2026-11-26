//! Tests that are run as part of the package build process.
mod content_test;
mod run_test;
mod serialize_test;

/// Directory (relative to the prefix) where rattler-build stores the files
/// needed by the package's command tests.
const TEST_FILES_DIR: &str = "etc/conda/test-files";

pub use content_test::PackageContentsTestExt;
pub use run_test::{TestConfiguration, TestError, run_test};
pub(crate) use serialize_test::write_test_files;
