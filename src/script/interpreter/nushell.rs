use std::{collections::HashMap, path::PathBuf};

use itertools::Itertools;
use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{ActivationVariables, Activator, PathModificationBehavior},
    shell::{self, Shell, ShellEnum},
};

use crate::script::{ExecutionArgs, run_process_with_replacements};

use super::{Interpreter, InterpreterError, find_interpreter};

pub(crate) struct NuShellInterpreter;

const NUSHELL_PREAMBLE: &str = r#"
## Start of bash preamble
if not ("CONDA_BUILD" in $env) {
    source-env ((script_path))
}

## End of preamble
"#;

impl Interpreter for NuShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let host_shell_type = ShellEnum::default();
        let nushell = ShellEnum::NuShell(Default::default());

        // Create a map of environment variables to pass to the shell script
        let mut activation_variables: HashMap<_, _> = HashMap::from_iter(args.env_vars.clone());

        // Read some of the current environment variables
        let current_path = std::env::var(nushell.path_var(&args.execution_platform))
            .map(|p| std::env::split_paths(&p).collect_vec())
            .ok();
        let current_conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

        // Run the activation script for the host environment.
        let activation_vars = ActivationVariables {
            conda_prefix: current_conda_prefix,
            path: current_path,
            path_modification_behavior: PathModificationBehavior::default(),
        };

        let host_prefix_activator = Activator::from_path(
            &args.run_prefix,
            host_shell_type.clone(),
            args.execution_platform,
        )
        .unwrap();

        let host_activation_variables = host_prefix_activator
            .run_activation(activation_vars, None)
            .unwrap();

        // Overwrite the current environment variables with the one from the activated host environment.
        activation_variables.extend(host_activation_variables);

        // If there is a build environment run the activation script for that environment and extend
        // the activation variables with the new environment variables.
        if let Some(build_prefix) = &args.build_prefix {
            let build_prefix_activator =
                Activator::from_path(build_prefix, host_shell_type, args.execution_platform)
                    .unwrap();

            let activation_vars = ActivationVariables {
                conda_prefix: None,
                path: activation_variables
                    .get(nushell.path_var(&args.execution_platform))
                    .map(|path| std::env::split_paths(&path).collect()),
                path_modification_behavior: PathModificationBehavior::default(),
            };

            let build_activation = build_prefix_activator
                .run_activation(activation_vars, None)
                .unwrap();

            activation_variables.extend(build_activation);
        }

        // Construct a shell script with the activation variables.
        let mut shell_script = shell::ShellScript::new(shell::NuShell, Platform::current());
        for (k, v) in activation_variables.iter() {
            shell_script.set_env_var(k, v).unwrap();
        }
        let script = shell_script
            .contents()
            .expect("failed to construct shell script");

        let build_env_path = args.work_dir.join("build_env.nu");
        let build_script_path = args.work_dir.join("conda_build.nu");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble =
            NUSHELL_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
        let script = format!("{}\n{}", preamble, args.script.script());
        tokio::fs::write(&build_script_path, script).await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();

        let nu_path =
            match find_interpreter("nu", args.build_prefix.as_ref(), &args.execution_platform) {
                Ok(Some(path)) => path,
                _ => {
                    return Err(InterpreterError::ExecutionFailed(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "NuShell executable not found in PATH",
                    )));
                }
            }
            .to_string_lossy()
            .to_string();

        let cmd_args = [nu_path.as_str(), build_script_path_str.as_str()];

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("$((var))"),
            None,
        )
        .await?;

        if !output.status.success() {
            let status_code = output.status.code().unwrap_or(1);
            tracing::error!("Script failed with status {}", status_code);
            tracing::error!("Work directory: '{}'", args.work_dir.display());
            return Err(InterpreterError::ExecutionFailed(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Script failed".to_string(),
            )));
        }

        Ok(())
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        find_interpreter("nu", build_prefix, platform)
    }
}
