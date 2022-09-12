use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn create_environment(
    specs: &[String],
    channels: &[String],
    prefix: PathBuf,
) -> Result<std::process::ExitStatus, std::io::Error> {
    let mut mm_cmd = Command::new("micromamba");

    mm_cmd.arg("create");

    for c in channels.iter() {
        mm_cmd.arg("-c");
        mm_cmd.arg(c);
    }

    mm_cmd.args([OsStr::new("-p"), prefix.as_os_str()]);

    // For debugging purposes only
    mm_cmd.args(["--offline"]);

    mm_cmd.args(specs);

    let res = mm_cmd.stdin(Stdio::null()).status();
    if res.is_err() {
        print!("{:?}", &res);
    }

    res
}
