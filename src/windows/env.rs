// def windows_vars(m, get_default, prefix):
//     """This is setting variables on a dict that is part of the get_default function"""
//     # We have gone for the clang values here.
//     win_arch = "i386" if str(m.config.host_arch) == "32" else "amd64"
//     win_msvc = "19.0.0"
//     library_prefix = join(prefix, "Library")
//     drive, tail = m.config.host_prefix.split(":")
//     get_default("SCRIPTS", join(prefix, "Scripts"))
//     get_default("LIBRARY_PREFIX", library_prefix)
//     get_default("LIBRARY_BIN", join(library_prefix, "bin"))
//     get_default("LIBRARY_INC", join(library_prefix, "include"))
//     get_default("LIBRARY_LIB", join(library_prefix, "lib"))
//     get_default(
//         "CYGWIN_PREFIX", "".join(("/cygdrive/", drive.lower(), tail.replace("\\", "/")))
//     )
//     # see https://en.wikipedia.org/wiki/Environment_variable#Default_values
//     get_default("ALLUSERSPROFILE")
//     get_default("APPDATA")
//     get_default("CommonProgramFiles")
//     get_default("CommonProgramFiles(x86)")
//     get_default("CommonProgramW6432")
//     get_default("COMPUTERNAME")
//     get_default("ComSpec")
//     get_default("HOMEDRIVE")
//     get_default("HOMEPATH")
//     get_default("LOCALAPPDATA")
//     get_default("LOGONSERVER")
//     get_default("NUMBER_OF_PROCESSORS")
//     get_default("PATHEXT")
//     get_default("ProgramData")
//     get_default("ProgramFiles")
//     get_default("ProgramFiles(x86)")
//     get_default("ProgramW6432")
//     get_default("PROMPT")
//     get_default("PSModulePath")
//     get_default("PUBLIC")
//     get_default("SystemDrive")
//     get_default("SystemRoot")
//     get_default("TEMP")
//     get_default("TMP")
//     get_default("USERDOMAIN")
//     get_default("USERNAME")
//     get_default("USERPROFILE")
//     get_default("windir")
//     # CPU data, see https://github.com/conda/conda-build/issues/2064
//     get_default("PROCESSOR_ARCHITEW6432")
//     get_default("PROCESSOR_ARCHITECTURE")
//     get_default("PROCESSOR_IDENTIFIER")
//     get_default("BUILD", win_arch + "-pc-windows-" + win_msvc)
//     for k in os.environ.keys():
//         if re.match("VS[0-9]{2,3}COMNTOOLS", k):
//             get_default(k)
//         elif re.match("VS[0-9]{4}INSTALLDIR", k):
//             get_default(k)

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
        let val = std::env::var(var).unwrap_or_default();
        vars.insert(var.to_string(), val);
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
