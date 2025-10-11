//! Source code tracking for error reporting with miette
//!
//! This module is only available when the `miette` feature is enabled.

#[cfg(feature = "miette")]
use miette::{MietteError, MietteSpanContents, SourceSpan, SpanContents};
#[cfg(feature = "miette")]
use std::path::PathBuf;
#[cfg(feature = "miette")]
use std::{path::Path, sync::Arc};

#[cfg(feature = "miette")]
use std::fmt::Debug;

/// A helper trait that provides source code for rattler-build-recipe.
///
/// This trait is useful for error reporting to provide information about the
/// source code for diagnostics.
#[cfg(feature = "miette")]
pub trait SourceCode: Debug + Clone + AsRef<str> + miette::SourceCode {}

#[cfg(feature = "miette")]
impl<T: Debug + Clone + AsRef<str> + miette::SourceCode> SourceCode for T {}

/// The contents of a specific source file together with the name of the source
/// file.
///
/// The name of the source file is used to identify the source file in error
/// messages.
#[cfg(feature = "miette")]
#[derive(Debug, Clone)]
pub struct Source {
    /// The name of the source.
    pub name: String,
    /// The source code.
    pub code: Arc<str>,
    /// The actual path to the source file.
    pub path: PathBuf,
}

#[cfg(feature = "miette")]
impl Source {
    /// Constructs a new instance from a string with a name
    pub fn from_string(name: String, contents: String) -> Self {
        Self {
            name: name.clone(),
            code: Arc::from(contents.as_str()),
            path: PathBuf::from(name),
        }
    }

    /// Constructs a new instance by loading the source code from a file.
    /// The current working dir is used as root path.
    pub fn from_path(path: &Path) -> std::io::Result<Self> {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self::from_rooted_path(&current_dir, path.to_path_buf())
    }

    /// Constructs a new instance by loading the source code from a file.
    ///
    /// The root directory is used to calculate the relative path of the source
    /// which is then used as the name of the source.
    pub fn from_rooted_path(root_dir: &Path, path: PathBuf) -> std::io::Result<Self> {
        let relative_path = pathdiff::diff_paths(&path, root_dir);
        let name = relative_path
            .as_deref()
            .map(|path| path.as_os_str())
            .or_else(|| path.file_name())
            .map(|p| p.to_string_lossy())
            .unwrap_or_default()
            .into_owned();

        let contents = std::fs::read_to_string(&path)?;
        Ok(Self {
            name,
            code: Arc::from(contents.as_str()),
            path,
        })
    }
}

#[cfg(feature = "miette")]
impl AsRef<str> for Source {
    fn as_ref(&self) -> &str {
        self.code.as_ref()
    }
}

#[cfg(feature = "miette")]
impl miette::SourceCode for Source {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        let inner_contents =
            self.as_ref()
                .read_span(span, context_lines_before, context_lines_after)?;
        let contents = MietteSpanContents::new_named(
            self.name.clone(),
            inner_contents.data(),
            *inner_contents.span(),
            inner_contents.line(),
            inner_contents.column(),
            inner_contents.line_count(),
        );
        Ok(Box::new(contents))
    }
}
