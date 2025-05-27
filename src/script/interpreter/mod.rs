mod bash;
mod cmd_exe;
mod nushell;
mod perl;
mod python;
mod r;

use std::path::PathBuf;

pub(crate) use bash::BashInterpreter;
pub(crate) use cmd_exe::CmdExeInterpreter;
pub(crate) use nushell::NuShellInterpreter;
pub(crate) use perl::PerlInterpreter;
pub(crate) use python::PythonInterpreter;
pub(crate) use r::RInterpreter;

use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{
        ActivationError, ActivationVariables, Activator, PathModificationBehavior,
        prefix_path_entries,
    },
    shell::{self, Shell},
};

use super::ExecutionArgs;

/// The error type for the interpreter
#[derive(Debug, thiserror::Error)]
pub enum InterpreterError {
    /// This error is returned when running in debug mode
    #[error("Debugging information: {0}")]
    Debug(String),

    /// This error is returned when the script execution fails or the interpreter is not found
    #[error("IO Error: {0}")]
    ExecutionFailed(#[from] std::io::Error),
}

pub const BASH_PREAMBLE: &str = r#"#!/bin/bash
## Start of bash preamble
if [ -z ${CONDA_BUILD+x} ]; then
    source "((script_path))"
fi
## End of preamble
"#;

pub const CMDEXE_PREAMBLE: &str = r#"
@chcp 65001 > nul
@echo on
IF "%CONDA_BUILD%" == "" (
    @rem special behavior from conda-build for Windows
    call "((script_path))"
)
@rem re-enable echo because the activation scripts might have messed with it
@echo on
"#;

fn find_interpreter(
    name: &str,
    build_prefix: Option<&PathBuf>,
    platform: &Platform,
) -> Result<Option<PathBuf>, which::Error> {
    let exe_name = format!("{}{}", name, std::env::consts::EXE_SUFFIX);

    let path = std::env::var("PATH").unwrap_or_default();
    if let Some(build_prefix) = build_prefix {
        let mut prepend_path = prefix_path_entries(build_prefix, platform)
            .into_iter()
            .collect::<Vec<_>>();
        prepend_path.extend(std::env::split_paths(&path));
        return Ok(
            which::which_in_global(exe_name, std::env::join_paths(prepend_path).ok())?.next(),
        );
    }

    Ok(which::which_in_global(exe_name, Some(path))?.next())
}

pub trait Interpreter {
    fn get_script<T: Shell + Copy + 'static>(
        &self,
        args: &ExecutionArgs,
        shell_type: T,
    ) -> Result<String, ActivationError> {
        let mut shell_script = shell::ShellScript::new(shell_type, Platform::current());
        for (k, v) in args.env_vars.iter() {
            shell_script.set_env_var(k, v)?;
        }
        let host_prefix_activator =
            Activator::from_path(&args.run_prefix, shell_type, args.execution_platform)?;

        let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

        let activation_vars = ActivationVariables {
            conda_prefix,
            path: None,
            path_modification_behavior: PathModificationBehavior::Prepend,
        };

        let host_activation = host_prefix_activator.activation(activation_vars)?;

        if let Some(build_prefix) = &args.build_prefix {
            let build_prefix_activator =
                Activator::from_path(build_prefix, shell_type, args.execution_platform)?;

            let activation_vars = ActivationVariables {
                conda_prefix: None,
                path: None,
                path_modification_behavior: PathModificationBehavior::Prepend,
            };

            let build_activation = build_prefix_activator.activation(activation_vars)?;
            shell_script.append_script(&host_activation.script);
            shell_script.append_script(&build_activation.script);
        } else {
            shell_script.append_script(&host_activation.script);
        }

        Ok(shell_script.contents()?)
    }

    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError>;

    #[allow(dead_code)]
    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error>;
}
