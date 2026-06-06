//! Prefix activation script generation.
//!
//! This module creates the shell-specific `build_env.*` content that activates
//! the run prefix and, when present, the build prefix before user script code
//! runs.

use std::collections::HashMap;

use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{ActivationError, ActivationVariables, Activator, PathModificationBehavior},
    shell::{self, Shell},
};

use crate::execution::ExecutionArgs;

/// Returns the shell-specific activation script sourced/called by the native wrapper.
pub(crate) fn activation_script<T: Shell + Clone + 'static>(
    args: &ExecutionArgs,
    shell_type: T,
) -> Result<String, ActivationError> {
    let mut shell_script = shell::ShellScript::new(shell_type.clone(), Platform::current());
    for (k, v) in args.env_vars.iter() {
        shell_script.set_env_var(k, v)?;
    }
    // Re-entrancy marker: this way the preamble sources this file
    // once and nested shells skip re-sourcing it.
    shell_script.set_env_var("CONDA_BUILD", "1")?;
    let host_prefix_activator = Activator::from_path(
        &args.run_prefix,
        shell_type.clone(),
        args.execution_platform,
    )?;

    // Do not pass the host CONDA_PREFIX to the activation. When
    // CONDA_PREFIX is set (e.g. running inside a pixi/conda env), the
    // activator generates deactivation scripts for that environment.
    let current_env = std::env::vars().collect::<HashMap<_, _>>();
    let activation_vars = ActivationVariables {
        conda_prefix: None,
        path: None,
        path_modification_behavior: PathModificationBehavior::Prepend,
        current_env: current_env.clone(),
    };

    let host_activation = host_prefix_activator.activation(activation_vars)?;

    if let Some(build_prefix) = &args.build_prefix {
        let build_prefix_activator =
            Activator::from_path(build_prefix, shell_type.clone(), args.execution_platform)?;
        let activation_vars = ActivationVariables {
            conda_prefix: None,
            path: None,
            path_modification_behavior: PathModificationBehavior::Prepend,
            current_env: current_env.clone(),
        };

        let build_activation = build_prefix_activator.activation(activation_vars)?;
        shell_script.append_script(&host_activation.script);
        shell_script.append_script(&build_activation.script);
    } else {
        shell_script.append_script(&host_activation.script);
    }

    Ok(shell_script.contents()?)
}
