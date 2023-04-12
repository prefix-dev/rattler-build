use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use goblin::pe::{header::DOS_MAGIC, PE};
use scroll::Pread;

#[derive(Debug)]
struct Dll {
    /// Path to the DLL
    path: PathBuf,
    /// Libraries that this DLL depends on
    libraries: HashSet<String>,
}

/// List of System DLLs that are allowed to be linked against.
const WIN_ALLOWLIST: &[&str] = &[
    "ADVAPI32.dll",
    "bcrypt.dll",
    "COMCTL32.dll",
    "COMDLG32.dll",
    "CRYPT32.dll",
    "dbghelp.dll",
    "GDI32.dll",
    "IMM32.dll",
    "KERNEL32.dll",
    "NETAPI32.dll",
    "ole32.dll",
    "OLEAUT32.dll",
    "PSAPI.DLL",
    "RPCRT4.dll",
    "SHELL32.dll",
    "USER32.dll",
    "USERENV.dll",
    "WINHTTP.dll",
    "WS2_32.dll",
    "ntdll.dll",
    "msvcrt.dll",
];

#[derive(Debug, thiserror::Error)]
pub enum DllParseError {
    #[error("failed to read the DLL file: {0}")]
    ReadFailed(#[from] std::io::Error),

    #[error("failed to parse the DLL file: {0}")]
    ParseFailed(#[from] goblin::error::Error),
}

impl Dll {
    /// Check if the file is a DLL (PE) file.
    fn test_file(path: &Path) -> Result<bool, std::io::Error> {
        let mut file = File::open(path)?;
        let mut buf: [u8; 2] = [0; 2];
        file.read_exact(&mut buf)?;
        let signature = buf
            .pread_with::<u16>(0, scroll::LE)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(DOS_MAGIC == signature)
    }

    /// Parse a DLL file and return an object that contains the path to the DLL and the list of
    /// libraries it depends on.
    fn new(path: &Path) -> Result<Self, DllParseError> {
        let mut buffer = Vec::new();
        let mut file = File::open(path)?;
        file.read_to_end(&mut buffer)?;
        let pe = PE::parse(&buffer)?;
        Ok(Self {
            path: path.to_path_buf(),
            libraries: pe.libraries.iter().map(|s| s.to_string()).collect(),
        })
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_dll() {
//         let path = PathBuf::from(
//             "/Users/wolfv/Downloads/libmamba-0.24.0-h81a967f_1/Library/bin/libmamba.dll",
//         );
//         let dll = Dll::new(&path).unwrap();
//         println!("{:?}", dll);
//         // assert_eq!(dll.path, PathBuf::from("C:\\Windows\\System32\\kernel32.dll"));
//         // assert_eq!(dll.imports.len(), 0);
//     }
// }
