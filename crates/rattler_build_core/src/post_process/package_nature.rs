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
};

/// The nature of a package
#[derive(Debug, PartialEq, Eq)]
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
}

/// A mapping from shared library filenames to the package that provides them.
///
/// This is used as a fallback during overlinking checks when the staging
/// cache's host dependencies are not physically installed in the prefix.
/// Instead of requiring files on disk, this allows name-based attribution
/// of libraries to packages.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryNameMap {
    /// Maps library filenames (e.g. "libz.so.1", "libz.1.dylib") to the
    /// package name that provides them.
    pub library_to_package: HashMap<String, PackageName>,
}

impl LibraryNameMap {
    /// Build a `LibraryNameMap` from a `PrefixInfo` by extracting the
    /// filenames of all files that look like shared objects.
    pub(crate) fn from_prefix_info(prefix_info: &PrefixInfo) -> Self {
        let mut library_to_package = HashMap::new();

        for (path, package_name) in &prefix_info.path_to_package {
            if is_dso(&path.path)
                && let Some(file_name) = path.path.file_name()
            {
                library_to_package.insert(
                    file_name.to_string_lossy().to_string(),
                    package_name.clone(),
                );
            }
        }

        Self { library_to_package }
    }

    /// Look up a library path by extracting its filename and checking the map.
    /// Returns the package name if found.
    ///
    /// Handles various library path forms:
    /// - Plain filenames: `libz.so.1`
    /// - macOS @rpath references: `@rpath/libz.1.dylib`
    /// - Full or relative paths: `lib/libz.so.1`
    pub fn find_package(&self, library: &Path) -> Option<PackageName> {
        let path_str = library.to_string_lossy();

        // Strip @rpath/ or @loader_path/ prefixes (macOS)
        let stripped = path_str
            .strip_prefix("@rpath/")
            .or_else(|| path_str.strip_prefix("@loader_path/"))
            .unwrap_or(&path_str);

        // Try the stripped path directly (handles plain filenames)
        if let Some(pkg) = self.library_to_package.get(stripped) {
            return Some(pkg.clone());
        }

        // Try just the filename component
        let file_name = Path::new(stripped).file_name()?.to_string_lossy();

        self.library_to_package.get(file_name.as_ref()).cloned()
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.library_to_package.is_empty()
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
