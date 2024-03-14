use miette::IntoDiagnostic;
use std::env;
use std::path::Path;
use std::process::Command;

/// Runs the user's default editor to edit content.
pub fn run_editor(path: &Path) -> miette::Result<()> {
    let editor = env::var("EDITOR").into_diagnostic()?;
    Command::new(editor).arg(path).status().into_diagnostic()?;
    Ok(())
}
