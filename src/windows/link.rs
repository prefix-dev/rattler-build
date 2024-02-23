#![allow(dead_code)]
use std::{
    collections::HashSet,
    fs::File,
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
        let file = File::open(path).expect("Failed to open the ELF file");
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let signature = mmap[0..2]
            .pread_with::<u16>(0, scroll::LE)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(DOS_MAGIC == signature)
    }

    /// Parse a DLL file and return an object that contains the path to the DLL and the list of
    /// libraries it depends on.
    fn new(path: &Path) -> Result<Self, DllParseError> {
        let file = File::open(path).expect("Failed to open the Mach-O binary");
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let pe = PE::parse(&mmap)?;
        Ok(Self {
            path: path.to_path_buf(),
            libraries: pe.libraries.iter().map(|s| s.to_string()).collect(),
        })
    }
}
