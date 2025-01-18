//! Provides a trait for source code that can be used for error reporting. See
//! [`SourceCode`].

use std::fmt::Debug;

/// A helper trait that provides source code for rattler-build.
///
/// This trait is useful for error reporting to provide information about the
/// source code for diagnostics.
pub trait SourceCode: Debug + Clone + AsRef<str> + miette::SourceCode {}
impl<T: Debug + Clone + AsRef<str> + miette::SourceCode> SourceCode for T {}
