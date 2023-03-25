use anyhow::anyhow;
use anyhow::Result;
use std::{path::Path, process::Command};

pub fn index(output_folder: &Path) -> Result<()> {
    // call conda index in subprocess
    let mut cmd = Command::new("conda");
    cmd.arg("index");
    cmd.arg(output_folder);
    let output = cmd.output().expect("Failed to execute conda index");
    if !output.status.success() {
        tracing::error!("Failed to execute conda index");
        tracing::error!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
        tracing::error!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow!("Failed to execute conda index"));
    }
    Ok(())
}
