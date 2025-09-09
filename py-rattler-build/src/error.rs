use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::fmt;

pyo3::create_exception!(rattler_build, PyRattlerBuildError, PyRuntimeError);

#[derive(Debug)]
pub enum RattlerBuildError {
    PlatformParse(String),
    ChannelPriority(String),
    PackageFormat(String),
    UrlParse(String),
    Auth(String),
    Upload(String),
    RecipeParse(String),
    Variant(String),
    Json(String),
    Io(String),
    Other(String),
}

impl fmt::Display for RattlerBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RattlerBuildError::PlatformParse(msg) => write!(f, "Platform parse error: {}", msg),
            RattlerBuildError::ChannelPriority(msg) => write!(f, "Channel priority error: {}", msg),
            RattlerBuildError::PackageFormat(msg) => write!(f, "Package format error: {}", msg),
            RattlerBuildError::UrlParse(msg) => write!(f, "URL parse error: {}", msg),
            RattlerBuildError::Auth(msg) => write!(f, "Authentication error: {}", msg),
            RattlerBuildError::Upload(msg) => write!(f, "Upload error: {}", msg),
            RattlerBuildError::RecipeParse(msg) => write!(f, "Recipe parse error: {}", msg),
            RattlerBuildError::Variant(msg) => write!(f, "Variant error: {}", msg),
            RattlerBuildError::Json(msg) => write!(f, "JSON error: {}", msg),
            RattlerBuildError::Io(msg) => write!(f, "IO error: {}", msg),
            RattlerBuildError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for RattlerBuildError {}

impl From<RattlerBuildError> for PyErr {
    fn from(error: RattlerBuildError) -> Self {
        PyRattlerBuildError::new_err(error.to_string())
    }
}

impl From<url::ParseError> for RattlerBuildError {
    fn from(error: url::ParseError) -> Self {
        RattlerBuildError::UrlParse(error.to_string())
    }
}

impl From<serde_json::Error> for RattlerBuildError {
    fn from(error: serde_json::Error) -> Self {
        RattlerBuildError::Json(error.to_string())
    }
}

impl From<std::io::Error> for RattlerBuildError {
    fn from(error: std::io::Error) -> Self {
        RattlerBuildError::Io(error.to_string())
    }
}

impl From<miette::Report> for RattlerBuildError {
    fn from(error: miette::Report) -> Self {
        RattlerBuildError::Other(error.to_string())
    }
}
