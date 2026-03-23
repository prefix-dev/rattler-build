use std::path::PathBuf;
use std::str::FromStr;

use ::rattler_build::{
    debug as core_debug, metadata::Output, source::create_patch, tool_configuration::Configuration,
};
use pyo3::prelude::*;
use rattler_conda_types::{ChannelUrl, NamedChannelOrUrl};

use crate::build::output_from_rendered_variant;
use crate::error::RattlerBuildError;
use crate::render::PyRenderedVariant;
use crate::run_async_task;
use crate::tool_config::PyToolConfiguration;
use crate::tracing_subscriber;

/// A debug session that holds a fully set-up build environment.
#[pyclass(name = "DebugSession")]
pub struct PyDebugSession {
    output: Output,
    setup_log: Vec<String>,
    channels: Vec<ChannelUrl>,
    tool_config: Configuration,
}

#[pymethods]
impl PyDebugSession {
    /// The work directory where the build script and sources live.
    #[getter]
    fn work_dir(&self) -> PathBuf {
        self.output.build_configuration.directories.work_dir.clone()
    }

    /// The host prefix (where host dependencies are installed).
    #[getter]
    fn host_prefix(&self) -> PathBuf {
        self.output
            .build_configuration
            .directories
            .host_prefix
            .clone()
    }

    /// The build prefix (where build dependencies are installed).
    #[getter]
    fn build_prefix(&self) -> PathBuf {
        self.output
            .build_configuration
            .directories
            .build_prefix
            .clone()
    }

    /// Path to the build script (conda_build.sh or conda_build.bat).
    #[getter]
    fn build_script(&self) -> PathBuf {
        let work_dir = &self.output.build_configuration.directories.work_dir;
        if cfg!(windows) {
            work_dir.join("conda_build.bat")
        } else {
            work_dir.join("conda_build.sh")
        }
    }

    /// Path to the build environment script (build_env.sh or build_env.bat).
    #[getter]
    fn build_env_script(&self) -> PathBuf {
        let work_dir = &self.output.build_configuration.directories.work_dir;
        if cfg!(windows) {
            work_dir.join("build_env.bat")
        } else {
            work_dir.join("build_env.sh")
        }
    }

    /// The build directory (parent of work, host_env, build_env).
    #[getter]
    fn build_dir(&self) -> PathBuf {
        self.output
            .build_configuration
            .directories
            .build_dir
            .clone()
    }

    /// The output directory where packages are written.
    #[getter]
    fn output_dir(&self) -> PathBuf {
        self.output
            .build_configuration
            .directories
            .output_dir
            .clone()
    }

    /// The recipe directory.
    #[getter]
    fn recipe_dir(&self) -> PathBuf {
        self.output
            .build_configuration
            .directories
            .recipe_dir
            .clone()
    }

    /// Log messages captured during setup.
    #[getter]
    fn setup_log(&self) -> Vec<String> {
        self.setup_log.clone()
    }

    /// Run the build script and capture stdout/stderr.
    ///
    /// Returns a tuple of (exit_code, stdout, stderr).
    #[pyo3(signature = (trace=false))]
    fn run_script(&self, trace: bool) -> PyResult<(i32, String, String)> {
        let work_dir = &self.output.build_configuration.directories.work_dir;
        let result =
            core_debug::run_build_script(work_dir, trace).map_err(RattlerBuildError::Io)?;
        Ok((result.exit_code, result.stdout, result.stderr))
    }

    /// Add packages to a host or build environment.
    ///
    /// Returns a list of package names that were installed.
    #[pyo3(signature = (specs, environment="host", channels=None))]
    fn add_packages(
        &self,
        specs: Vec<String>,
        environment: &str,
        channels: Option<Vec<String>>,
    ) -> PyResult<Vec<String>> {
        let prefix_dir = match environment {
            "host" => &self.output.build_configuration.directories.host_prefix,
            "build" => &self.output.build_configuration.directories.build_prefix,
            _ => {
                return Err(RattlerBuildError::Other(format!(
                    "unknown environment '{}', expected 'host' or 'build'",
                    environment
                ))
                .into());
            }
        };

        let channels: Vec<ChannelUrl> = if let Some(ch) = channels {
            let channel_config = &self.tool_config.channel_config;
            ch.iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::Channel(e.to_string()))
                        .and_then(|named| {
                            named.into_base_url(channel_config).map_err(|e| {
                                RattlerBuildError::Other(format!("channel error: {}", e))
                            })
                        })
                })
                .collect::<Result<_, _>>()?
        } else {
            self.channels.clone()
        };

        run_async_task(async {
            core_debug::add_packages_to_prefix(
                environment,
                prefix_dir,
                &specs,
                &channels,
                &self.tool_config,
            )
            .await
        })?;

        Ok(specs)
    }

    /// Create a patch from changes in the work directory.
    ///
    /// Returns the patch content as a string (when dry_run=True) or an empty
    /// string on success.
    #[pyo3(signature = (name="changes", output_dir=None, overwrite=false, exclude=None, add=None, include=None, dry_run=false))]
    #[allow(clippy::too_many_arguments)]
    fn create_patch(
        &self,
        name: &str,
        output_dir: Option<PathBuf>,
        overwrite: bool,
        exclude: Option<Vec<String>>,
        add: Option<Vec<String>>,
        include: Option<Vec<String>>,
        dry_run: bool,
    ) -> PyResult<String> {
        let work_dir = &self.output.build_configuration.directories.work_dir;
        let output_dir = output_dir.or_else(|| {
            Some(
                self.output
                    .build_configuration
                    .directories
                    .recipe_dir
                    .clone(),
            )
        });

        create_patch::create_patch(
            work_dir,
            name,
            overwrite,
            output_dir.as_deref(),
            &exclude.unwrap_or_default(),
            &add.unwrap_or_default(),
            &include.unwrap_or_default(),
            dry_run,
        )
        .map_err(|e| RattlerBuildError::Other(e.to_string()))?;

        Ok(String::new())
    }

    /// Read and return the build script contents.
    fn read_build_script(&self) -> PyResult<String> {
        let path = self.build_script();
        fs_err::read_to_string(&path).map_err(|e| RattlerBuildError::Io(e).into())
    }

    fn __repr__(&self) -> String {
        format!(
            "DebugSession(work_dir='{}')",
            self.output
                .build_configuration
                .directories
                .work_dir
                .display()
        )
    }
}

/// Create a debug session from a rendered variant.
///
/// Sets up the full build environment (resolves dependencies, fetches sources,
/// installs environments, creates build script) without running the build.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (rendered_variant, tool_config=None, output_dir=None, channels=None, no_build_id=true, progress_callback=None))]
pub fn create_debug_session_py(
    py: Python<'_>,
    rendered_variant: PyRenderedVariant,
    tool_config: Option<PyToolConfiguration>,
    output_dir: Option<PathBuf>,
    channels: Option<Vec<String>>,
    no_build_id: bool,
    progress_callback: Option<Py<PyAny>>,
) -> PyResult<PyDebugSession> {
    let tool_config = tool_config
        .map(|tc| tc.inner)
        .unwrap_or_else(|| Configuration::builder().finish());

    let output_dir = output_dir.unwrap_or_else(|| {
        std::env::temp_dir()
            .join(format!("rattler_build_{:x}", rand_hash()))
            .join("output")
    });

    // Ensure output directory exists
    fs_err::create_dir_all(&output_dir).map_err(RattlerBuildError::Io)?;

    let channels_named: Vec<NamedChannelOrUrl> = channels
        .unwrap_or_else(|| vec!["conda-forge".to_string()])
        .iter()
        .map(|c| {
            NamedChannelOrUrl::from_str(c).map_err(|e| RattlerBuildError::Channel(e.to_string()))
        })
        .collect::<Result<_, _>>()?;

    let channels_urls: Vec<ChannelUrl> = channels_named
        .iter()
        .map(|c| c.clone().into_base_url(&tool_config.channel_config))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| RattlerBuildError::Other(format!("channel error: {}", e)))?;

    let output = output_from_rendered_variant(
        &rendered_variant,
        &tool_config,
        &output_dir,
        &channels_named,
        no_build_id,
        None,
        true, // no_include_recipe
        None, // recipe_path
        None, // exclude_newer
    )?;

    // Run setup with log capture
    let (setup_result, log_buffer) =
        tracing_subscriber::with_log_capture(progress_callback, || {
            run_async_task(async { output.setup_debug_environment(&tool_config).await })
        });

    let captured_logs = log_buffer
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();

    let output = match setup_result {
        Ok(output) => output,
        Err(err) => {
            return Err(crate::error::build_error_with_log(
                py,
                err.to_string(),
                captured_logs,
            ));
        }
    };

    Ok(PyDebugSession {
        output,
        setup_log: captured_logs,
        channels: channels_urls,
        tool_config,
    })
}

/// Generate a simple random-ish hash for temp directory names.
fn rand_hash() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    hasher.finish()
}

/// Register the debug module with Python.
pub fn register_debug_module(py: Python<'_>, parent: &Bound<'_, PyModule>) -> PyResult<()> {
    let m = PyModule::new(py, "debug")?;
    m.add_class::<PyDebugSession>()?;
    m.add_function(wrap_pyfunction!(create_debug_session_py, &m)?)?;
    parent.add_submodule(&m)?;
    Ok(())
}
