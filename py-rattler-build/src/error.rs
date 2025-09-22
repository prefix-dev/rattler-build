use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use thiserror::Error;

create_exception!(rattler_build, PyRattlerBuildError, PyException);
create_exception!(rattler_build, PyPlatformParseError, PyRattlerBuildError);
create_exception!(rattler_build, PyChannelPriorityError, PyRattlerBuildError);
create_exception!(rattler_build, PyPackageFormatError, PyRattlerBuildError);
create_exception!(rattler_build, PyUrlParseError, PyRattlerBuildError);
create_exception!(rattler_build, PyAuthError, PyRattlerBuildError);
create_exception!(rattler_build, PyUploadError, PyRattlerBuildError);
create_exception!(rattler_build, PyRecipeParseError, PyRattlerBuildError);
create_exception!(rattler_build, PyVariantError, PyRattlerBuildError);
create_exception!(rattler_build, PyJsonError, PyRattlerBuildError);
create_exception!(rattler_build, PyIoError, PyRattlerBuildError);

#[derive(Error, Debug)]
pub enum RattlerBuildError {
    #[error("Platform parse error: {0}")]
    PlatformParse(#[from] rattler_conda_types::ParsePlatformError),

    #[error("Channel priority error: {0}")]
    ChannelPriority(String),

    #[error("Package format error: {0}")]
    PackageFormat(String),

    #[error("URL parse error")]
    UrlParse(#[from] url::ParseError),

    #[error("Authentication error: {0}")]
    Auth(#[from] rattler_networking::authentication_storage::AuthenticationStorageError),

    #[error("Upload error: {0}")]
    Upload(String),

    #[error("Recipe parse error: {0}")]
    RecipeParse(String),

    #[error("Variant error: {0}")]
    Variant(String),

    #[error("JSON error")]
    Json(#[from] serde_json::Error),

    #[error("IO error")]
    Io(#[from] std::io::Error),

    #[error("Error: {0}")]
    Other(String),
}

impl From<RattlerBuildError> for PyErr {
    fn from(error: RattlerBuildError) -> Self {
        match error {
            RattlerBuildError::PlatformParse(e) => PyPlatformParseError::new_err(e.to_string()),
            RattlerBuildError::ChannelPriority(msg) => PyChannelPriorityError::new_err(msg),
            RattlerBuildError::PackageFormat(msg) => PyPackageFormatError::new_err(msg),
            RattlerBuildError::UrlParse(e) => PyUrlParseError::new_err(e.to_string()),
            RattlerBuildError::Auth(e) => PyAuthError::new_err(e.to_string()),
            RattlerBuildError::Upload(msg) => PyUploadError::new_err(msg),
            RattlerBuildError::RecipeParse(msg) => PyRecipeParseError::new_err(msg),
            RattlerBuildError::Variant(msg) => PyVariantError::new_err(msg),
            RattlerBuildError::Json(e) => PyJsonError::new_err(e.to_string()),
            RattlerBuildError::Io(e) => PyIoError::new_err(e.to_string()),
            RattlerBuildError::Other(e) => PyRattlerBuildError::new_err(e.to_string()),
        }
    }
}

impl From<miette::Report> for RattlerBuildError {
    fn from(error: miette::Report) -> Self {
        RattlerBuildError::Other(error.to_string())
    }
}

pub(crate) fn register_exceptions(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RattlerBuildError", py.get_type::<PyRattlerBuildError>())?;
    m.add("PlatformParseError", py.get_type::<PyPlatformParseError>())?;
    m.add(
        "ChannelPriorityError",
        py.get_type::<PyChannelPriorityError>(),
    )?;
    m.add("PackageFormatError", py.get_type::<PyPackageFormatError>())?;
    m.add("UrlParseError", py.get_type::<PyUrlParseError>())?;
    m.add("AuthError", py.get_type::<PyAuthError>())?;
    m.add("UploadError", py.get_type::<PyUploadError>())?;
    m.add("RecipeParseError", py.get_type::<PyRecipeParseError>())?;
    m.add("VariantError", py.get_type::<PyVariantError>())?;
    m.add("JsonError", py.get_type::<PyJsonError>())?;
    m.add("IoError", py.get_type::<PyIoError>())?;
    Ok(())
}
