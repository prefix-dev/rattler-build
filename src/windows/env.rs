use std::{collections::HashMap, path::Path};

use rattler_conda_types::Platform;

pub fn default_env_vars(prefix: &Path, target_platform: &Platform) -> HashMap<String, String> {
    let win_arch = match target_platform {
        Platform::Win32 => "i386",
        Platform::Win64 => "amd64",
        // TODO: Is this correct?
        Platform::WinArm64 => "arm64",
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

    // vars.insert("CYGWIN_PREFIX", "".join(("/cygdrive/", drive.lower(), tail.replace("\\", "/"))));

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
        std::env::var("BUILD").unwrap_or(format!("{}-pc-windows-{}", win_arch, win_msvc)),
    );

    // TODO
    // get_default(
    //     "CYGWIN_PREFIX", "".join(("/cygdrive/", drive.lower(), tail.replace("\\", "/")))
    // )

    // for k in os.environ.keys():
    //     if re.match("VS[0-9]{2,3}COMNTOOLS", k):
    //         get_default(k)
    //     elif re.match("VS[0-9]{4}INSTALLDIR", k):
    //         get_default(k)

    vars
}
