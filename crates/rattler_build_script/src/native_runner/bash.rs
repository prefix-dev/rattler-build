use std::fmt::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use rattler_shell::shell::{self, Shell};

use super::{CommandSpec, NativeShellRunner};
use crate::{ExecutionContext, PrefixLayout};

pub(crate) struct BashNativeRunner;

impl NativeShellRunner for BashNativeRunner {
    fn shell(&self) -> shell::ShellEnum {
        shell::Bash::default().into()
    }

    fn default_interpreter(&self) -> &'static str {
        "bash"
    }

    fn preamble(&self, activation_script_path: &std::path::Path) -> String {
        format!(
            r#"#!/usr/bin/env bash
set -e
## Start of bash preamble
if [ -z ${{CONDA_BUILD+x}} ]; then
    source "{}"
fi
## End of preamble
# Trace each command as it runs so a failing line is visible (see #2264).
# Placed after activation so the sourced environment setup is not traced.
set -x
"#,
            activation_script_path.to_string_lossy()
        )
    }

    fn command_to_run_script(
        &self,
        build_script_path: &Path,
        _context: &ExecutionContext,
    ) -> CommandSpec {
        CommandSpec::new("bash", [build_script_path.to_string_lossy().into_owned()])
    }

    fn replacements_template(&self) -> &'static str {
        "$((var))"
    }

    /// Subshell scope. Inherited `set -e` aborts on failure, so no guard is
    /// needed — but it must stay a bare statement (chaining `||`/`&&` would
    /// suppress `set -e`).
    fn scope_section(
        &self,
        label: Option<&str>,
        env: &IndexMap<String, String>,
        body: &str,
    ) -> Result<String, std::io::Error> {
        let shell = shell::Bash::default();
        let mut out = String::new();
        if let Some(label) = label {
            let _ = writeln!(out, "# === {label} ===");
        }
        out.push_str("(\n");
        for (key, value) in env {
            shell
                .set_env_var(&mut out, key, value)
                .map_err(std::io::Error::other)?;
        }
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push(')');
        Ok(out)
    }

    /// Returns reproduction instructions for the failed bash wrapper script.
    fn debug_info(&self, work_dir: &Path, context: &ExecutionContext) -> String {
        let mut output = String::new();

        output.push_str("\nScript execution failed.\n\n");
        output.push_str(&format!("  Work directory: {}\n", work_dir.display()));
        output.push_str(&format!("  Prefix: {}\n", context.host().path().display()));

        if context.layout() == PrefixLayout::Separate {
            output.push_str(&format!(
                "  Build prefix: {}\n",
                context.build().path().display()
            ));
        } else {
            output.push_str("  Build prefix: None\n");
        }

        output.push_str("\nTo run the script manually, use the following command:\n\n");
        output.push_str(&format!("  cd {:?} && ./conda_build.sh\n\n", work_dir));
        output.push_str("To run commands interactively in the build environment:\n\n");
        output.push_str(&format!("  cd {:?} && source build_env.sh", work_dir));

        output
    }
}
