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

use std::{
    collections::HashSet,
    ops::Sub,
    path::{Path, PathBuf},
};

use rattler_conda_types::PrefixRecord;

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
    /// R or Python library with DSOs
    PluginLibraryR,
    PluginLibraryPython,
    PluginLibraryPythonAndR,
    /// Pure R or Python library without any DSOs
    InterpretedLibraryR,
    InterpretedLibraryPython,
    InterpretedLibraryPythonAndR,
}

pub fn is_dso(file: &Path) -> bool {
    let ext = file.extension();
    let ext = ext.unwrap_or_default().to_string_lossy().to_string();
    matches!(ext.as_str(), "so" | "dylib" | "dll" | "pyd")
}

impl PackageNature {
    pub fn from_prefix_record(prefix_record: &PrefixRecord) -> Self {
        if prefix_record
            .repodata_record
            .package_record
            .name
            .as_normalized()
            == "python"
        {
            return PackageNature::InterpreterPython;
        } else if prefix_record
            .repodata_record
            .package_record
            .name
            .as_normalized()
            == "r-base"
        {
            return PackageNature::InterpreterR;
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
