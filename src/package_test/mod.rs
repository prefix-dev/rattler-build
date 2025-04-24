//! Tests that are run as part of the package build process.
mod content_test;
mod run_test;
mod serialize_test;

pub use run_test::{TestConfiguration, TestError, run_test};
pub(crate) use serialize_test::write_test_files;
