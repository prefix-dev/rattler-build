#![allow(dead_code)]
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use fs_err::File;
use goblin::pe::PE;
use rattler_build_recipe::stage1::GlobVec;
use rattler_conda_types::Platform;
use rattler_shell::activation::prefix_path_entries;

use crate::post_process::relink::{RelinkError, Relinker};

#[derive(Debug)]
pub struct Dll {
    /// Path to the DLL
    path: PathBuf,
    /// Libraries that this DLL depends on
    libraries: HashSet<PathBuf>,
}

/// File extensions that are known to be PE (Portable Executable) files on Windows.
const PE_EXTENSIONS: &[&str] = &["dll", "exe", "pyd", "sys", "ocx", "drv", "cpl", "scr", "efi"];

/// Returns true if the file extension indicates a potential PE file.
fn has_pe_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| PE_EXTENSIONS.iter().any(|pe_ext| ext.eq_ignore_ascii_case(pe_ext)))
}

/// List of System DLLs that are allowed to be linked against.
pub const WIN_ALLOWLIST: &[&str] = &[
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
    "SHLWAPI.dll",
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

/// The DOS header magic number that every PE file starts with ("MZ").
const PE_MAGIC: [u8; 2] = [0x4D, 0x5A];

impl Dll {
    /// Try to parse a PE file and create a Dll analyzer, returning None for
    /// non-PE files.
    ///
    /// Applies two cheap filters before the full parse:
    ///  1. File extension – skip obviously non-PE files without any I/O.
    ///  2. MZ magic – read two bytes, same approach Linux/macOS use for
    ///     ELF/Mach-O magic.  This is what keeps `.pyc`, `.npz` etc. from
    ///     ever reaching the full goblin parse.
    pub fn try_new(path: &Path) -> Result<Option<Self>, RelinkError> {
        if !has_pe_extension(path) {
            return Ok(None);
        }

        let file = File::open(path)?;
        let mmap = unsafe { memmap2::Mmap::map(&file)? };

        // Quick magic check – every PE file starts with the DOS "MZ" header.
        if mmap.len() < 2 || mmap[..2] != PE_MAGIC {
            return Ok(None);
        }

        match PE::parse(&mmap) {
            Ok(pe) => Ok(Some(Self {
                path: path.to_path_buf(),
                libraries: pe.libraries.iter().map(PathBuf::from).collect(),
            })),
            Err(e) => {
                // The file starts with MZ but isn't a well-formed PE.
                // Log and skip rather than failing the build.
                tracing::debug!("[relink/windows] Skipping invalid PE file {path:?}: {e}");
                Ok(None)
            }
        }
    }
}

impl Relinker for Dll {
    fn test_file(path: &Path) -> Result<bool, RelinkError> {
        match Self::try_new(path)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    fn new(path: &Path) -> Result<Self, RelinkError> {
        match Self::try_new(path)? {
            Some(dll) => Ok(dll),
            None => Err(RelinkError::ParseError(goblin::error::Error::Malformed(
                "Not a valid PE file".to_string(),
            ))),
        }
    }

    fn libraries(&self) -> HashSet<PathBuf> {
        self.libraries.clone()
    }

    fn resolve_libraries(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
    ) -> HashMap<PathBuf, Option<PathBuf>> {
        let mut result = HashMap::new();
        for lib in &self.libraries {
            if WIN_ALLOWLIST.iter().any(|&sys_dll| {
                lib.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case(sys_dll))
                    .unwrap_or(false)
            }) {
                continue;
            }

            let dll_name = lib.file_name().unwrap_or_default();

            // 1. Check in the same directory as the original DLL
            let path_in_prefix = self
                .path
                .strip_prefix(prefix)
                .expect("DLL path should be in prefix");
            let path = encoded_prefix.join(path_in_prefix);
            let same_dir = path.parent().map(|p| p.join(dll_name));

            if let Some(path) = same_dir
                && path.exists()
            {
                result.insert(lib.clone(), Some(path));
                continue;
            }

            // 2. Check all directories in the search path
            let mut found = false;
            let path_entries = prefix_path_entries(encoded_prefix, &Platform::Win64);
            let path = std::env::var("PATH").unwrap_or_default();
            let search_dirs = path_entries
                .into_iter()
                .chain(std::env::split_paths(&path))
                .collect::<Vec<_>>();

            for search_dir in search_dirs {
                let potential_path = search_dir.join(dll_name);
                if potential_path.exists() {
                    result.insert(lib.clone(), Some(potential_path));
                    found = true;
                    break;
                }
            }

            if !found {
                // If not found anywhere, keep the original name but mark as None
                result.insert(lib.clone(), None);
            }
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
        _rpath_allowlist: &GlobVec,
        _system_tools: &crate::system_tools::SystemTools,
    ) -> Result<(), crate::post_process::relink::RelinkError> {
        // On Windows, we don't need to relink anything
        Ok(())
    }
}

#[cfg(test)]
#[cfg(target_os = "windows")]
mod tests {
    use super::*;
    use fs_err as fs;
    use std::io::Write;
    use std::path::Path;

    const TEST_DLL_DIR: &str = "test-data/binary_files/windows/zstd/Library/bin";

    #[test]
    fn test_dll_detection() -> Result<(), RelinkError> {
        let dll_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(TEST_DLL_DIR)
            .join("zstd.dll");
        assert!(Dll::test_file(&dll_path)?);

        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files/windows");
        fs::create_dir_all(&prefix)?;
        let invalid_file = prefix.join("invalid.dll");
        let mut file = File::create(&invalid_file)?;
        file.write_all(&[0x00, 0x00])?;
        assert!(!Dll::test_file(&invalid_file)?);
        fs::remove_file(&invalid_file)?;

        Ok(())
    }

    #[test]
    fn test_dll_dependencies() -> Result<(), RelinkError> {
        let dll_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(TEST_DLL_DIR)
            .join("zstd.dll");
        let dll = Dll::new(&dll_path)?;

        let libraries = dll.libraries();

        assert!(!libraries.is_empty(), "Expected DLL to have dependencies");

        let has_kernel32 = libraries.iter().any(|lib| {
            lib.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("KERNEL32.dll"))
                .unwrap_or(false)
        });
        assert!(has_kernel32, "Expected KERNEL32.dll dependency");

        let resolved = dll.resolve_libraries(&dll_path, &dll_path);
        assert!(
            !resolved.iter().any(|(lib, _)| lib
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("KERNEL32.dll"))
                .unwrap_or(false)),
            "System DLLs should be filtered out during resolution"
        );

        Ok(())
    }

    #[test]
    fn test_system_dll_filtering() {
        let test_dlls = vec![
            "KERNEL32.dll",
            "kernel32.dll",
            "C:\\Windows\\System32\\KERNEL32.dll",
            "D:\\Some\\Path\\kernel32.dll",
            "custom.dll",
            "myapp.dll",
        ];

        for dll in test_dlls {
            let path = PathBuf::from(dll);
            let is_system = WIN_ALLOWLIST.iter().any(|&sys_dll| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case(sys_dll))
                    .unwrap_or(false)
            });

            match dll.to_lowercase().as_str() {
                "kernel32.dll"
                | "c:\\windows\\system32\\kernel32.dll"
                | "d:\\some\\path\\kernel32.dll" => {
                    assert!(is_system, "Expected {} to be identified as system DLL", dll);
                }
                _ => {
                    assert!(
                        !is_system,
                        "Expected {} to NOT be identified as system DLL",
                        dll
                    );
                }
            }
        }
    }
}
