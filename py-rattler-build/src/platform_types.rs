use pyo3::prelude::*;
use pyo3::types::PyList;
use rattler_build::types::PlatformWithVirtualPackages;
use rattler_conda_types::{GenericVirtualPackage, Platform};
use std::str::FromStr;

use crate::error::RattlerBuildError;

/// Python wrapper for PlatformWithVirtualPackages
#[pyclass(name = "PlatformWithVirtualPackages")]
#[derive(Clone)]
pub struct PyPlatformWithVirtualPackages {
    pub(crate) inner: PlatformWithVirtualPackages,
}

#[pymethods]
impl PyPlatformWithVirtualPackages {
    /// Create a new platform with virtual packages
    #[new]
    #[pyo3(signature = (platform=None))]
    fn new(platform: Option<String>) -> PyResult<Self> {
        let platform = if let Some(p) = platform {
            p.parse::<Platform>()
                .map_err(|e| RattlerBuildError::Other(format!("Invalid platform: {}", e)))?
        } else {
            Platform::current()
        };

        let inner = PlatformWithVirtualPackages::detect_for_platform(
            platform,
            &rattler_virtual_packages::VirtualPackageOverrides::from_env(),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Failed to detect virtual packages: {}", e)))?;

        Ok(Self { inner })
    }

    /// Get the platform as a string
    #[getter]
    fn platform(&self) -> String {
        self.inner.platform.to_string()
    }

    /// Get the virtual packages
    #[getter]
    fn virtual_packages(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let list = PyList::empty(py);
        for vp in &self.inner.virtual_packages {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("name", vp.name.as_source())?;
            dict.set_item("version", vp.version.to_string())?;
            // build_string is a String, not an Option
            dict.set_item("build_string", &vp.build_string)?;
            list.append(dict)?;
        }
        Ok(list.into())
    }

    fn __repr__(&self) -> String {
        format!(
            "PlatformWithVirtualPackages(platform='{}', virtual_packages_count={})",
            self.inner.platform,
            self.inner.virtual_packages.len()
        )
    }
}

/// Python wrapper for Platform enum
#[pyclass(name = "Platform")]
#[derive(Clone)]
pub struct PyPlatform {
    pub(crate) inner: Platform,
}

#[pymethods]
impl PyPlatform {
    /// Create a platform from a string
    #[new]
    fn new(platform: &str) -> PyResult<Self> {
        let inner = platform
            .parse::<Platform>()
            .map_err(|e| RattlerBuildError::Other(format!("Invalid platform: {}", e)))?;
        Ok(Self { inner })
    }

    /// Get the current platform
    #[staticmethod]
    fn current() -> Self {
        Self {
            inner: Platform::current(),
        }
    }

    /// Check if this is a NoArch platform
    fn is_noarch(&self) -> bool {
        self.inner == Platform::NoArch
    }

    /// Get the platform as a string
    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("Platform('{}')", self.inner)
    }

    /// Check equality
    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    /// Hash for use in sets/dicts
    fn __hash__(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        format!("{:?}", self.inner).hash(&mut hasher);
        hasher.finish()
    }
}

/// Register the platform_types module with Python
pub fn register_platform_types_module(py: Python<'_>, parent: &Bound<'_, PyModule>) -> PyResult<()> {
    let m = PyModule::new(py, "platform_types")?;
    m.add_class::<PyPlatformWithVirtualPackages>()?;
    m.add_class::<PyPlatform>()?;
    parent.add_submodule(&m)?;
    Ok(())
}
