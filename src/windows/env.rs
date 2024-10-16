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
        format!(
            "/cygdrive/{}/{}",
            drive_letter.to_lowercase(),
            rest.map(|c| c.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        )
    } else {
        // fallback to `c` if no drive letter is found
        format!(
            "/cygdrive/c/{}",
            path.iter()
                .map(|c| c.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        )
    }
}

pub fn default_env_vars(
    prefix: &Path,
    target_platform: &Platform,
) -> HashMap<String, Option<String>> {
    let library_prefix = prefix.join("Library");
    let mut vars = HashMap::<String, Option<String>>::new();
    vars.insert(
        "SCRIPTS".to_string(),
        Some(prefix.join("Scripts").display().to_string()),
    );
    vars.insert(
        "LIBRARY_PREFIX".to_string(),
        Some(library_prefix.display().to_string()),
    );
    vars.insert(
        "LIBRARY_BIN".to_string(),
        Some(library_prefix.join("bin").display().to_string()),
    );
    let library_lib = library_prefix.join("lib");
    let library_inc = library_prefix.join("include");
    vars.insert(
        "LIBRARY_INC".to_string(),
        Some(library_inc.display().to_string()),
    );
    vars.insert(
        "LIBRARY_LIB".to_string(),
        Some(library_lib.display().to_string()),
    );

    // This adds the LIB and INCLUDE vars. It would not be entirely correct if someone
    // overwrites the LIBRARY_LIB or LIBRARY_INCLUDE variables from the variants.yaml
    // but I think for now this is fine.
    let lib_var = std::env::var("LIB").ok().unwrap_or_default();
    let include_var = std::env::var("INCLUDE").ok().unwrap_or_default();
    vars.insert(
        "LIB".to_string(),
        Some(format!("{};{}", library_lib.display(), lib_var)),
    );
    vars.insert(
        "INCLUDE".to_string(),
        Some(format!("{};{}", library_inc.display(), include_var)),
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
        vars.insert(var.to_string(), std::env::var(var).ok());
    }

    // Do we need to get these from the variant configuration?
    let win_msvc = "19.0.0";

    let win_arch = match target_platform {
        Platform::Win32 => "i386",
        Platform::Win64 => "amd64",
        Platform::WinArm64 => "arm64",
        Platform::NoArch => "noarch",
        _ => panic!("Non windows platform passed to windows env vars"),
    };

    vars.insert(
        "BUILD".to_string(),
        Some(
            std::env::var("BUILD")
                .unwrap_or_else(|_| format!("{}-pc-windows-{}", win_arch, win_msvc)),
        ),
    );

    vars.insert("CYGWIN_PREFIX".to_string(), Some(to_cygdrive(prefix)));

    let re_vs_comntools = Regex::new(r"^VS[0-9]{2,3}COMNTOOLS$").unwrap();
    let re_vs_installdir = Regex::new(r"^VS[0-9]{4}INSTALLDIR$").unwrap();

    for (key, val) in std::env::vars() {
        if re_vs_comntools.is_match(&key) || re_vs_installdir.is_match(&key) {
            vars.insert(key, Some(val));
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
