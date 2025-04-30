#![allow(dead_code)]
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
};

use fs_err::File;

use goblin::pe::{PE, header::DOS_MAGIC};
use rattler_conda_types::Platform;
use rattler_shell::activation::prefix_path_entries;
use scroll::Pread;

use crate::{
    post_process::relink::{RelinkError, Relinker},
    recipe::parser::GlobVec,
};

#[derive(Debug)]
pub struct Dll {
    /// Path to the DLL
    path: PathBuf,
    /// Libraries that this DLL depends on
    libraries: HashSet<PathBuf>,
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

            if let Some(path) = same_dir {
                if path.exists() {
                    result.insert(lib.clone(), Some(path));
                    continue;
                }
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
