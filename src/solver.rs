use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn create_environment(
    specs: Vec<&str>,
    channels: Vec<&str>,
    prefix: PathBuf,
) -> Result<std::process::ExitStatus, std::io::Error> {
    let mut mm_cmd = Command::new("micromamba");

    mm_cmd.arg("create");

    for c in channels.into_iter() {
        mm_cmd.arg("-c");
        mm_cmd.arg(c);
    }

    mm_cmd.args([OsStr::new("-p"), prefix.as_os_str()]);

    mm_cmd.args(specs);

    let res = mm_cmd.stdin(Stdio::null()).status();
    if res.is_err() {
        print!("{:?}", &res);
    }
    return res;
}
