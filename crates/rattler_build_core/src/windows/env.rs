use std::{
    collections::HashMap,
    path::{Component::*, Path, Prefix::Disk},
};

use rattler_build_script::RuntimeEnv;
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

pub fn default_env_vars_target(
    prefix: &Path,
    runtime: &RuntimeEnv,
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
    let lib_var = runtime.var("LIB").unwrap_or_default();
    let include_var = runtime.var("INCLUDE").unwrap_or_default();
    vars.insert(
        "LIB".to_string(),
        Some(format!("{};{}", library_lib.display(), lib_var)),
    );
    vars.insert(
        "INCLUDE".to_string(),
        Some(format!("{};{}", library_inc.display(), include_var)),
    );

    vars.insert("CYGWIN_PREFIX".to_string(), Some(to_cygdrive(prefix)));
    vars
}

pub fn default_env_vars_build(
    build_platform: &Platform,
    runtime: &RuntimeEnv,
) -> HashMap<String, Option<String>> {
    let mut vars = HashMap::<String, Option<String>>::new();
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
        vars.insert(var.to_string(), runtime.var(var).map(str::to_owned));
    }

    // Do we need to get these from the variant configuration?
    let win_msvc = "19.0.0";

    let win_arch = match build_platform {
        Platform::Win32 => "i386",
        Platform::Win64 => "amd64",
        Platform::WinArm64 => "arm64",
        Platform::NoArch => "noarch",
        _ => panic!("Non windows platform passed to windows env vars"),
    };

    vars.insert(
        "BUILD".to_string(),
        Some(
            runtime
                .var("BUILD")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{}-pc-windows-{}", win_arch, win_msvc)),
        ),
    );

    let re_vs_comntools = Regex::new(r"^VS[0-9]{2,3}COMNTOOLS$").unwrap();
    let re_vs_installdir = Regex::new(r"^VS[0-9]{4}INSTALLDIR$").unwrap();

    for (key, val) in runtime.vars() {
        let normalized_key = key.to_ascii_uppercase();
        if re_vs_comntools.is_match(&normalized_key) || re_vs_installdir.is_match(&normalized_key) {
            vars.insert(key.to_owned(), Some(val.to_owned()));
        }
    }

    vars
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn build_vars_use_the_injected_runtime_environment() {
        let runtime = RuntimeEnv::for_test(Platform::Win64)
            .with_var("vs140comntools", "C:\\VS140")
            .with_var("BUILD", "injected-build");
        let vars = default_env_vars_build(&Platform::Win64, &runtime);

        assert_eq!(
            vars.get("vs140comntools")
                .and_then(|value| value.as_deref()),
            Some("C:\\VS140")
        );
        assert_eq!(
            vars.get("BUILD").and_then(|value| value.as_deref()),
            Some("injected-build")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_cygdrive() {
        let path = std::path::Path::new("C:\\Users\\user\\Documents");
        let cygdrive = super::to_cygdrive(path);
        assert_eq!(cygdrive, "/cygdrive/c/Users/user/Documents");
    }
}
