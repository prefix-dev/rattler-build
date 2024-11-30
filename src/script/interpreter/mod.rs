mod bash;
mod cmd_exe;
mod nushell;
mod perl;
mod python;

use std::path::PathBuf;

pub(crate) use bash::BashInterpreter;
pub(crate) use cmd_exe::CmdExeInterpreter;
pub(crate) use nushell::NuShellInterpreter;
pub(crate) use perl::PerlInterpreter;
pub(crate) use python::PythonInterpreter;

use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{prefix_path_entries, ActivationError, ActivationVariables, Activator},
    shell::{self, Shell},
};

use super::ExecutionArgs;

const DEBUG_HELP : &str  = "To debug the build, run it manually in the work directory (execute the `./conda_build.sh` or `conda_build.bat` script)";

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

        let current_path = std::env::var(shell_type.path_var(&args.execution_platform))
            .ok()
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
        let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

        let activation_vars = ActivationVariables {
            conda_prefix,
            path: current_path,
            path_modification_behavior: Default::default(),
        };

        let host_activation = host_prefix_activator.activation(activation_vars)?;

        if let Some(build_prefix) = &args.build_prefix {
            let build_prefix_activator =
                Activator::from_path(build_prefix, shell_type, args.execution_platform)?;

            let activation_vars = ActivationVariables {
                conda_prefix: None,
                path: Some(host_activation.path.clone()),
                path_modification_behavior: Default::default(),
            };

            let build_activation = build_prefix_activator.activation(activation_vars)?;
            shell_script.append_script(&host_activation.script);
            shell_script.append_script(&build_activation.script);
        } else {
            shell_script.append_script(&host_activation.script);
        }

        Ok(shell_script.contents()?)
    }

    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error>;

    #[allow(dead_code)]
    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error>;
}
