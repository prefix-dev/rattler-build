use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn create_environment(
    specs: &[String],
    channels: &[String],
    prefix: PathBuf,
    platform: &str,
) -> Result<std::process::ExitStatus, std::io::Error> {
    // check for MAMBA_EXE env var
    // if it exists, use that instead of micromamba
    let mut mm_cmd = if let Ok(mamba_exe) = std::env::var("MAMBA_EXE") {
        Command::new(mamba_exe)
    } else {
        Command::new("micromamba")
    };

    mm_cmd.arg("create");

    for c in channels.iter() {
        mm_cmd.arg("-c");
        mm_cmd.arg(c);
    }

    mm_cmd.args([OsStr::new("-p"), prefix.as_os_str()]);
    mm_cmd.args(["--platform", platform]);
    mm_cmd.args(specs);

    let res = mm_cmd.stdin(Stdio::null()).status();
    if res.is_err() {
        print!("{:?}", &res);
    }

    res
}
