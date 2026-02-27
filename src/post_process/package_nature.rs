//! Determine the "nature" of a package based on all the shared objects found in the package

// "interpreter (Python)"
// | "interpreter (R)"
// | "run-exports library"
// | "dso library"
// | "plugin library (Python,R)"
// | "plugin library (Python)"
// | "plugin library (R)"
// | "interpreted library (Python,R)"
// | "interpreted library (Python)"
// | "interpreted library (R)"
// | "non-library"

use rattler_conda_types::{PackageName, PrefixRecord};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Sub,
    path::{Path, PathBuf},
    str::FromStr,
};

/// The nature of a package
#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub enum PackageNature {
    /// Libraries
    RunExportsLibrary,
    DSOLibrary,
    NonLibrary,
    /// Interpreters
    InterpreterR,
    InterpreterPython,
    InterpreterRuby,
    InterpreterNodeJs,
    InterpreterPowerShell,
    /// R or Python library with DSOs
    PluginLibraryR,
    PluginLibraryPython,
    PluginLibraryPythonAndR,
    /// Pure R or Python library without any DSOs
    InterpretedLibraryR,
    InterpretedLibraryPython,
    InterpretedLibraryPythonAndR,
}

/// Returns true if the given file is a dynamic shared object.
pub fn is_dso(file: &Path) -> bool {
    let ext = file.extension();
    let ext = ext.unwrap_or_default().to_string_lossy().to_string();
    matches!(ext.as_str(), "so" | "dylib" | "dll" | "pyd")
        || file
            .to_string_lossy()
            .split('.')
            .any(|part| part.to_lowercase() == "so")
}

impl PackageNature {
    /// Returns true if this package nature indicates the package provides
    /// shared objects that other packages may legitimately link against.
    /// This includes DSO libraries, run-exports libraries, interpreters
    /// (e.g. python provides python3XX.dll), and plugin libraries.
    pub fn provides_shared_objects(&self) -> bool {
        !matches!(
            self,
            PackageNature::NonLibrary
                | PackageNature::InterpretedLibraryPython
                | PackageNature::InterpretedLibraryR
                | PackageNature::InterpretedLibraryPythonAndR
        )
    }

    pub fn from_prefix_record(prefix_record: &PrefixRecord) -> Self {
        let package_name = prefix_record
            .repodata_record
            .package_record
            .name
            .as_normalized();

        if package_name == "python" {
            return PackageNature::InterpreterPython;
        } else if package_name == "r-base" {
            return PackageNature::InterpreterR;
        } else if package_name == "ruby" {
            return PackageNature::InterpreterRuby;
        } else if package_name == "nodejs" {
            return PackageNature::InterpreterNodeJs;
        } else if package_name == "powershell" {
            return PackageNature::InterpreterPowerShell;
        }
        let run_exports_json = PathBuf::from("info/run_exports.json");
        if prefix_record.files.contains(&run_exports_json) {
            return PackageNature::RunExportsLibrary;
        }

        let dsos = prefix_record
            .files
            .iter()
            .filter(|file| is_dso(file))
            .collect::<HashSet<_>>();

        let r_needle = vec!["lib", "R", "library"];

        let py_files = prefix_record
            .files
            .iter()
            .filter(|file| file.components().any(|c| c.as_os_str() == "site-packages"))
            .collect::<HashSet<_>>();

        let r_files = prefix_record
            .files
            .iter()
            .filter(|file| {
                let components = file.components().map(|c| c.as_os_str()).collect::<Vec<_>>();
                components.windows(3).any(|window| window == r_needle)
            })
            .collect::<HashSet<_>>();

        let py_dsos = py_files
            .intersection(&dsos)
            .cloned()
            .collect::<HashSet<_>>();
        let r_dsos = r_files.intersection(&dsos).cloned().collect::<HashSet<_>>();

        let mut nature = PackageNature::NonLibrary;
        if !dsos.is_empty() {
            if !dsos.sub(&py_dsos).sub(&r_dsos).is_empty() {
                nature = PackageNature::DSOLibrary;
            } else if !py_dsos.is_empty() && !r_dsos.is_empty() {
                nature = PackageNature::PluginLibraryPythonAndR;
            } else if !py_dsos.is_empty() {
                nature = PackageNature::PluginLibraryPython;
            } else if !r_dsos.is_empty() {
                nature = PackageNature::PluginLibraryR;
            }
        } else if !py_files.is_empty() && !r_files.is_empty() {
            nature = PackageNature::InterpretedLibraryPythonAndR;
        } else if !py_files.is_empty() {
            nature = PackageNature::InterpretedLibraryPython;
        } else if !r_files.is_empty() {
            nature = PackageNature::InterpretedLibraryR;
        }

        nature
    }
}

pub struct CaseInsensitivePathBuf {
    path: PathBuf,
}

impl CaseInsensitivePathBuf {
    fn normalize_path(&self) -> String {
        self.path
            .to_string_lossy()
            .to_lowercase()
            .replace('\\', "/")
    }
}

impl Hash for CaseInsensitivePathBuf {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.normalize_path().hash(state);
    }
}

impl From<PathBuf> for CaseInsensitivePathBuf {
    fn from(path: PathBuf) -> Self {
        CaseInsensitivePathBuf { path }
    }
}

impl PartialEq for CaseInsensitivePathBuf {
    fn eq(&self, other: &Self) -> bool {
        self.normalize_path() == other.normalize_path()
    }
}

impl Eq for CaseInsensitivePathBuf {}

#[derive(Default)]
pub(crate) struct PrefixInfo {
    pub package_to_nature: HashMap<PackageName, PackageNature>,
    pub path_to_package: HashMap<CaseInsensitivePathBuf, PackageName>,
}

impl PrefixInfo {
    pub fn from_prefix(prefix: &Path) -> Result<Self, std::io::Error> {
        let mut prefix_info = Self::default();

        let conda_meta = prefix.join("conda-meta");

        if conda_meta.exists() {
            for entry in conda_meta.read_dir()? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|v| v.to_str()) == Some("json") {
                    let record = PrefixRecord::from_path(path)?;
                    let package_nature = PackageNature::from_prefix_record(&record);
                    prefix_info.package_to_nature.insert(
                        record.repodata_record.package_record.name.clone(),
                        package_nature,
                    );

                    for file in record.files {
                        prefix_info.path_to_package.insert(
                            file.into(),
                            record.repodata_record.package_record.name.clone(),
                        );
                    }
                }
            }
        }

        Ok(prefix_info)
    }

    /// Merge cached prefix info (from a staging cache) into this PrefixInfo.
    /// Cached entries are only added if they don't already exist.
    pub fn merge_cached(&mut self, cached: &CachedPrefixInfo) {
        for (name_str, nature) in &cached.package_to_nature {
            if let Ok(name) = PackageName::from_str(name_str) {
                self.package_to_nature.entry(name).or_insert(nature.clone());
            }
        }
        for (path_str, name_str) in &cached.path_to_package {
            if let Ok(name) = PackageName::from_str(name_str) {
                let path_buf: CaseInsensitivePathBuf = PathBuf::from(path_str).into();
                self.path_to_package.entry(path_buf).or_insert(name);
            }
        }
    }
}

/// Serializable prefix info for storing in staging cache metadata.
/// Maps file paths to their owning packages and packages to their nature,
/// so that linking checks can attribute libraries without needing the
/// original conda-meta records installed.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CachedPrefixInfo {
    /// Maps file paths (relative to prefix) to package names
    pub path_to_package: HashMap<String, String>,
    /// Maps package names to their nature
    pub package_to_nature: HashMap<String, PackageNature>,
    /// Files produced by the staging cache build script (relative to prefix).
    /// These are build artifacts that will be split across sibling outputs.
    /// Used by linking checks to recognize libraries from sibling outputs.
    #[serde(default)]
    pub staging_prefix_files: Vec<String>,
}

impl CachedPrefixInfo {
    /// Build a CachedPrefixInfo from a PrefixInfo and the staging cache's
    /// prefix files (build artifacts).
    pub(crate) fn from_prefix_info(info: &PrefixInfo, staging_prefix_files: &[PathBuf]) -> Self {
        Self {
            path_to_package: info
                .path_to_package
                .iter()
                .map(|(path, name)| {
                    (
                        path.path.to_string_lossy().to_string(),
                        name.as_normalized().to_string(),
                    )
                })
                .collect(),
            package_to_nature: info
                .package_to_nature
                .iter()
                .map(|(name, nature)| (name.as_normalized().to_string(), nature.clone()))
                .collect(),
            staging_prefix_files: staging_prefix_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_is_dso() {
        let file_path = PathBuf::from("example.so");
        assert!(is_dso(&file_path));

        let file_path = PathBuf::from("libquadmath.dylib");
        assert!(is_dso(&file_path));

        let file_path = PathBuf::from("library.dll");
        assert!(is_dso(&file_path));

        let file_path = PathBuf::from("module.pyd");
        assert!(is_dso(&file_path));

        let file_path = PathBuf::from("not_dso.txt");
        assert!(!is_dso(&file_path));

        let file_path = PathBuf::from("lib/libquadmath.so.0.0.0");
        assert!(is_dso(&file_path));

        let file_path = PathBuf::from("bin/executable");
        assert!(!is_dso(&file_path));
    }
}
