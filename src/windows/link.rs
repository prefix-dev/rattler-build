#![allow(dead_code)]
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
};

use fs_err::File;

use goblin::pe::{header::DOS_MAGIC, PE};
use scroll::Pread;

use crate::post_process::relink::{RelinkError, Relinker};

#[derive(Debug)]
struct Dll {
    /// Path to the DLL
    path: PathBuf,
    /// Libraries that this DLL depends on
    libraries: HashSet<PathBuf>,
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

impl Relinker for Dll {
    fn test_file(path: &Path) -> Result<bool, RelinkError> {
        let mut file = File::open(path)?;
        let mut buf: [u8; 2] = [0; 2];
        file.read_exact(&mut buf)?;
        let signature = buf
            .pread_with::<u16>(0, scroll::LE)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(DOS_MAGIC == signature)
    }

    fn new(path: &Path) -> Result<Self, RelinkError> {
        let file = File::open(path)?;
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let pe = PE::parse(&mmap)?;
        Ok(Self {
            path: path.to_path_buf(),
            libraries: pe.libraries.iter().map(PathBuf::from).collect(),
        })
    }

    fn libraries(&self) -> HashSet<PathBuf> {
        self.libraries.clone()
    }

    fn resolve_libraries(
        &self,
        _prefix: &Path,
        _encoded_prefix: &Path,
    ) -> HashMap<PathBuf, Option<PathBuf>> {
        let mut result = HashMap::new();
        for lib in &self.libraries {
            result.insert(lib.clone(), Some(lib.clone()));
        }
        result
    }

    fn resolve_rpath(&self, _rpath: &Path, _prefix: &Path, _encoded_prefix: &Path) -> PathBuf {
        unimplemented!("This function does not make sense on Windows")
    }

    fn relink(
        &self,
        _prefix: &Path,
        _encoded_prefix: &Path,
        _custom_rpaths: &[String],
        _rpath_allowlist: Option<&globset::GlobSet>,
        _system_tools: &crate::system_tools::SystemTools,
    ) -> Result<(), crate::post_process::relink::RelinkError> {
        // On Windows, we don't need to relink anything
        Ok(())
    }
}
