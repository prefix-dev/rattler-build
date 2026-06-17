//! Prefix activation script generation.
//!
//! This module creates the shell-specific `build_env.*` content that activates
//! the run prefix and, when present, the build prefix before user script code
//! runs.

use std::collections::HashMap;

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
    let platform = args.runtime.platform();
    let mut shell_script = shell::ShellScript::new(shell_type.clone(), platform);
    for (k, v) in args.env_vars.iter() {
        shell_script.set_env_var(k, v)?;
    }
    // Re-entrancy marker: this way the preamble sources this file
    // once and nested shells skip re-sourcing it.
    shell_script.set_env_var("CONDA_BUILD", "1")?;
    let host_prefix_activator =
        Activator::from_path(&args.run_prefix, shell_type.clone(), platform)?;

    // Do not pass the host CONDA_PREFIX to the activation. When
    // CONDA_PREFIX is set (e.g. running inside a pixi/conda env), the
    // activator generates deactivation scripts for that environment.
    let current_env = args
        .runtime
        .vars()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect::<HashMap<_, _>>();
    let activation_vars = ActivationVariables {
        conda_prefix: None,
        path: None,
        path_modification_behavior: PathModificationBehavior::Prepend,
        current_env: current_env.clone(),
    };

    let host_activation = host_prefix_activator.activation(activation_vars)?;

    if let Some(build_prefix) = &args.build_prefix {
        let build_prefix_activator =
            Activator::from_path(build_prefix, shell_type.clone(), platform)?;
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use rattler_conda_types::Platform;
    use rattler_shell::shell;

    use crate::execution::{EnvironmentIsolation, ExecutionArgs, ResolvedScriptContents};
    use crate::runtime::RuntimeEnv;

    /// When a build prefix is present, both the run prefix and build prefix are
    /// activated and the generated script references both paths.
    #[test]
    fn activation_with_build_prefix_references_both_prefixes() {
        let tmp = tempfile::tempdir().unwrap();
        let run_prefix = tmp.path().join("run_prefix");
        let build_prefix = tmp.path().join("build_prefix");
        fs_err::create_dir_all(&run_prefix).unwrap();
        fs_err::create_dir_all(&build_prefix).unwrap();

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(String::new()),
            interpreter: None,
            env_vars: IndexMap::new(),
            secrets: IndexMap::new(),
            runtime: RuntimeEnv::for_test(Platform::current()),
            build_prefix: Some(build_prefix.clone()),
            run_prefix: run_prefix.clone(),
            work_dir: tmp.path().to_path_buf(),
            sandbox_config: None,
            env_isolation: EnvironmentIsolation::None,
        };

        let script = super::activation_script(&args, shell::Bash::default()).unwrap();

        // Both activations are appended. The Bash activator emits paths with
        // forward slashes (and may use a normalized temp root), so we assert on
        // the stable trailing component of each prefix rather than the full
        // platform display path. Distinct names prove both activations ran.
        assert!(
            script.contains("run_prefix"),
            "activation script must reference the run prefix, got:\n{script}"
        );
        assert!(
            script.contains("build_prefix"),
            "activation script must reference the build prefix, got:\n{script}"
        );
        // The build activation must be appended in addition to the run
        // activation: two separate PATH exports, one per prefix.
        let path_exports = script.matches("export PATH=").count();
        assert!(
            path_exports >= 2,
            "expected separate PATH exports for both prefixes, got {path_exports}:\n{script}"
        );
    }
}
