use std::{
    collections::HashMap,
    path::{Component::*, Path, Prefix::Disk},
};

use rattler_conda_types::Platform;
use regex::Regex;

fn get_drive_letter(path: &Path) -> Option<char> {
    path.components().find_map(|component| match component {
        Prefix(prefix_component) => match prefix_component.kind() {
            Disk(letter) => Some(letter as char),
            _ => None,
        },
        _ => None,
    })
}

fn to_cygdrive(path: &Path) -> String {
    if let Some(drive_letter) = get_drive_letter(path) {
        // skip first component, which is the drive letter and the `\` after it
        let rest = path.iter().skip(2);
        return format!(
            "/cygdrive/{}/{}",
            drive_letter.to_lowercase(),
            rest.map(|c| c.to_string_lossy()).collect::<Vec<_>>().join("/")
        );
    } else {
        return format!("/cygdrive/c/{}", path.iter().map(|c| c.to_string_lossy()).collect::<Vec<_>>().join("/"));
    }
}

pub fn default_env_vars(prefix: &Path, target_platform: &Platform) -> HashMap<String, String> {
    let win_arch = match target_platform {
        Platform::Win32 => "i386",
        Platform::Win64 => "amd64",
        // TODO: Is this correct?
        Platform::WinArm64 => "arm64",
        Platform::NoArch => "noarch",
        _ => panic!("Non windows platform passed to windows env vars"),
    };

    let win_msvc = "19.0.0";

    // let (drive, tail) = prefix.split(":");

    let library_prefix = prefix.join("Library");
    let mut vars = HashMap::<String, String>::new();
    vars.insert(
        "SCRIPTS".to_string(),
        prefix.join("Scripts").to_string_lossy().to_string(),
    );
    vars.insert(
        "LIBRARY_PREFIX".to_string(),
        library_prefix.to_string_lossy().to_string(),
    );
    vars.insert(
        "LIBRARY_BIN".to_string(),
        library_prefix.join("bin").to_string_lossy().to_string(),
    );
    vars.insert(
        "LIBRARY_INC".to_string(),
        library_prefix.join("include").to_string_lossy().to_string(),
    );
    vars.insert(
        "LIBRARY_LIB".to_string(),
        library_prefix.join("lib").to_string_lossy().to_string(),
    );

    let default_vars = vec![
        "ALLUSERSPROFILE",
        "APPDATA",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "CommonProgramW6432",
        "COMPUTERNAME",
        "ComSpec",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "LOGONSERVER",
        "NUMBER_OF_PROCESSORS",
        "PATHEXT",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "PROMPT",
        "PSModulePath",
        "PUBLIC",
        "SystemDrive",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "windir",
        // CPU data, see https://github.com/conda/conda-build/issues/2064
        "PROCESSOR_ARCHITEW6432",
        "PROCESSOR_ARCHITECTURE",
        "PROCESSOR_IDENTIFIER",
    ];

    for var in default_vars {
        if let Ok(val) = std::env::var(var) {
            vars.insert(var.to_string(), val);
        }
    }

    vars.insert(
        "BUILD".to_string(),
        std::env::var("BUILD").unwrap_or_else(|_| format!("{}-pc-windows-{}", win_arch, win_msvc)),
    );

    vars.insert("CYGWIN_PREFIX".to_string(), to_cygdrive(prefix));

    let re_vs_comntools = Regex::new(r"^VS[0-9]{2,3}COMNTOOLS$").unwrap();
    let re_vs_installdir = Regex::new(r"^VS[0-9]{4}INSTALLDIR$").unwrap();

    for (key, val) in std::env::vars() {
        if re_vs_comntools.is_match(&key) || re_vs_installdir.is_match(&key) {
            vars.insert(key, val);
        }
    }

    vars
}

#[cfg(test)]
mod test {
    #[cfg(target_os = "windows")]
    #[test]
    fn test_cygdrive() {
        let path = std::path::Path::new("C:\\Users\\user\\Documents");
        let cygdrive = super::to_cygdrive(path);
        assert_eq!(cygdrive, "/cygdrive/c/Users/user/Documents");
    }
}
