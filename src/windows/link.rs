use std::{path::{Path, PathBuf}, fs::File, io::Read, collections::HashSet};

use goblin::pe::{PE, import::Import};

#[derive(Debug)]
struct Dll {
    path: PathBuf,
    imports: HashSet<String>,
}

impl Dll {
    fn new(path: &Path) -> Result<Self, std::io::Error> {
        let mut imports = HashSet::new();
        let mut buffer = Vec::new();
        let mut file = File::open(&path).expect("Failed to open the DLL file");
        file.read_to_end(&mut buffer).expect("Failed to read the DLL file");
        let pe = PE::parse(&buffer).expect("Failed to parse the PE file");
        for import in pe.imports {
            println!("{:?}", import);
            imports.insert(import.dll.to_string());
        }
        Ok(Self { path: path.to_path_buf(), imports })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dll() {
        let path = PathBuf::from("/Users/wolfv/Downloads/libmamba-0.24.0-h81a967f_1/Library/bin/libmamba.dll");
        let dll = Dll::new(&path).unwrap();
        println!("{:?}", dll);
        // assert_eq!(dll.path, PathBuf::from("C:\\Windows\\System32\\kernel32.dll"));
        // assert_eq!(dll.imports.len(), 0);
    }
}