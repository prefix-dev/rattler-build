use rattler_build_recipe::stage1::build::LinkingCheckBehavior;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    metadata::Output,
    post_process::{package_nature::PrefixInfo, relink::RelinkError},
};
use crate::{
    post_process::{package_nature::PackageNature, relink},
    windows::link::WIN_ALLOWLIST,
};

use crate::render::resolved_dependencies::RunExportDependency;
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use rattler_conda_types::{PackageName, PrefixRecord};
use text_stub_library::TbdVersionedRecord;
use walkdir::WalkDir;

#[derive(thiserror::Error, Debug)]
pub enum LinkingCheckError {
    #[error("Error reading file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Shared object error: {0}")]
    SharedObject(String),

    #[error("Overlinking against: {package} (file: {file:?})")]
    Overlinking { package: PathBuf, file: PathBuf },

    #[error("Overdepending against: {package}")]
    Overdepending { package: PathBuf },

    #[error("failed to build glob from pattern")]
    GlobError(#[from] globset::Error),

    #[error("DSO list validation error: {0}")]
    DsoListValidation(String),
}

#[derive(Debug)]
struct PackageFile {
    pub file: PathBuf,
    pub linked_dsos: HashMap<PathBuf, PackageName>,
    pub shared_libraries: HashSet<PathBuf>,
}

#[derive(Debug)]
struct PackageLinkInfo {
    file: PathBuf,
    linked_packages: Vec<LinkedPackage>,
}

impl fmt::Display for PackageLinkInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "\n[{}] links against:",
            console::style(self.file.display()).white().bold()
        )?;
        for (i, package) in self.linked_packages.iter().enumerate() {
            let connector = if i != self.linked_packages.len() - 1 {
                " ├─"
            } else {
                " └─"
            };
            writeln!(f, "{connector} {package}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct LinkedPackage {
    name: PathBuf,
    link_origin: LinkOrigin,
}

#[derive(Debug)]
enum LinkOrigin {
    System,
    PackageItself,
    ForeignPackage(String),
    NotFound,
}

impl fmt::Display for LinkedPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.link_origin {
            LinkOrigin::System => {
                write!(f, "{} (system)", console::style(self.name.display()).dim())
            }
            LinkOrigin::PackageItself => {
                write!(
                    f,
                    "{} (package)",
                    console::style(self.name.display()).blue()
                )
            }
            LinkOrigin::ForeignPackage(package) => {
                write!(
                    f,
                    "{} ({})",
                    console::style(self.name.display()).green(),
                    console::style(package).italic()
                )
            }
            LinkOrigin::NotFound => {
                write!(f, "{}", console::style(self.name.display()).red())
            }
        }
    }
}

/// Returns the list of resolved run dependencies.
fn resolved_run_dependencies(
    output: &Output,
    package_to_nature_map: &HashMap<PackageName, PackageNature>,
) -> Vec<String> {
    output
        .finalized_dependencies
        .clone()
        .expect("failed to get the finalized dependencies")
        .run
        .depends
        .iter()
        .filter(|dep| {
            if let Some(RunExportDependency { from, .. }) = dep.as_run_export() {
                from != &String::from("build")
            } else {
                true
            }
        })
        .flat_map(|dep| {
            if let Some(rattler_conda_types::PackageNameMatcher::Exact(exact_name)) =
                &dep.spec().name
            {
                if let Some(nature) = package_to_nature_map.get(exact_name)
                    && nature != &PackageNature::DSOLibrary
                {
                    return None;
                }
                Some(exact_name.as_source().to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// A JSON configuration file that defines allow/deny patterns for system DLLs.
/// Used by CEP-28 to customize which DLLs are considered system libraries.
#[derive(Debug, serde::Deserialize)]
struct DsoList {
    version: u32,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
    subdir: String,
}

/// Validate that a dsolist glob pattern uses forward slashes and is either
/// an absolute path or starts with `**` (which the CEP considers absolute).
fn validate_dsolist_pattern(pattern: &str, file: &Path) -> Result<(), LinkingCheckError> {
    if pattern.contains('\\') {
        return Err(LinkingCheckError::DsoListValidation(format!(
            "Pattern '{}' in {} contains backslashes; use forward slashes for path separation",
            pattern,
            file.display()
        )));
    }

    // Patterns starting with ** are considered absolute per the CEP.
    // Otherwise, the path must be an absolute Windows path (drive letter followed by :/).
    let is_drive_letter_path = pattern.len() >= 3
        && pattern.as_bytes()[0].is_ascii_alphabetic()
        && pattern.as_bytes()[1] == b':'
        && pattern.as_bytes()[2] == b'/';

    if !pattern.starts_with("**") && !is_drive_letter_path {
        return Err(LinkingCheckError::DsoListValidation(format!(
            "Pattern '{}' in {} is not an absolute path; only Windows drive letter paths (e.g. C:/...) or patterns starting with ** are allowed",
            pattern,
            file.display()
        )));
    }

    Ok(())
}

/// Validate all dsolist JSON files that are about to be packaged.
/// This ensures that any `etc/conda-build/dsolists.d/*.json` files in the
/// package contents conform to the CEP-28 schema before they ship.
pub fn validate_dsolist_files(package_dir: &Path) -> Result<(), LinkingCheckError> {
    let dsolists_dir = package_dir.join("etc/conda-build/dsolists.d");

    let Ok(entries) = fs_err::read_dir(&dsolists_dir) else {
        return Ok(());
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = fs_err::read_to_string(&path).map_err(|e| {
            LinkingCheckError::DsoListValidation(format!(
                "Failed to read dsolist file {}: {}",
                path.display(),
                e
            ))
        })?;

        let dsolist: DsoList = serde_json::from_str(&content).map_err(|e| {
            LinkingCheckError::DsoListValidation(format!(
                "Failed to parse dsolist file {}: {}",
                path.display(),
                e
            ))
        })?;

        if dsolist.version != 1 {
            return Err(LinkingCheckError::DsoListValidation(format!(
                "Unsupported dsolist version {} in {} (only version 1 is supported)",
                dsolist.version,
                path.display()
            )));
        }

        for pattern in &dsolist.allow {
            validate_dsolist_pattern(pattern, &path)?;
        }
        for pattern in &dsolist.deny {
            validate_dsolist_pattern(pattern, &path)?;
        }
    }

    Ok(())
}

/// Load dsolist JSON files from the given prefix directory.
/// Returns collected (allow, deny) pattern lists from all matching files.
fn load_dsolists(
    prefix: &Path,
    subdir: &str,
) -> Result<(Vec<String>, Vec<String>), LinkingCheckError> {
    let dsolists_dir = prefix.join("etc/conda-build/dsolists.d");
    let mut allow_patterns = Vec::new();
    let mut deny_patterns = Vec::new();

    let Ok(entries) = fs_err::read_dir(&dsolists_dir) else {
        return Ok((allow_patterns, deny_patterns));
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = match fs_err::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read dsolist file {}: {}", path.display(), e);
                continue;
            }
        };

        let dsolist: DsoList = match serde_json::from_str(&content) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to parse dsolist file {}: {}", path.display(), e);
                continue;
            }
        };

        if dsolist.version != 1 {
            return Err(LinkingCheckError::DsoListValidation(format!(
                "Unsupported dsolist version {} in {} (only version 1 is supported)",
                dsolist.version,
                path.display()
            )));
        }

        if dsolist.subdir != subdir {
            continue;
        }

        // Validate all patterns before accepting them
        for pattern in &dsolist.allow {
            validate_dsolist_pattern(pattern, &path)?;
        }
        for pattern in &dsolist.deny {
            validate_dsolist_pattern(pattern, &path)?;
        }

        allow_patterns.extend(dsolist.allow);
        deny_patterns.extend(dsolist.deny);
    }

    Ok((allow_patterns, deny_patterns))
}

/// Expand dsolist patterns following conda-build's `_expand_dsolist` behavior.
/// - Patterns starting with `*` are kept as-is
/// - Absolute paths are converted to `**/{filename}` patterns
fn expand_dsolist(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .map(|p| {
            if p.starts_with('*') {
                p.clone()
            } else {
                let path = Path::new(p);
                if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                    format!("**/{filename}")
                } else {
                    p.clone()
                }
            }
        })
        .collect()
}

/// Extract install names from .tbd files in the given sysroot directory.
/// This parses the text-based stub files that macOS SDKs use to represent
/// dynamic libraries, extracting the actual runtime library paths.
fn extract_tbd_install_names(sysroot: &Path) -> Vec<String> {
    let mut install_names = Vec::new();

    for entry in WalkDir::new(sysroot)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "tbd"))
    {
        let Ok(content) = fs_err::read_to_string(entry.path()) else {
            continue;
        };

        let Ok(records) = text_stub_library::parse_str(&content) else {
            tracing::warn!("Failed to parse .tbd file at {}", entry.path().display());
            continue;
        };

        for record in records {
            let install_name = match &record {
                TbdVersionedRecord::V1(r) => &r.install_name,
                TbdVersionedRecord::V2(r) => &r.install_name,
                TbdVersionedRecord::V3(r) => &r.install_name,
                TbdVersionedRecord::V4(r) => &r.install_name,
            };

            if !install_name.is_empty() {
                install_names.push(install_name.clone());
            }
        }
    }

    install_names
}

fn add_osx_system_libs(
    output: &Output,
    system_libs: &mut GlobSetBuilder,
) -> Result<(), globset::Error> {
    // If CONDA_BUILD_SYSROOT is set, parse .tbd files to extract install names.
    // This matches conda-build's behavior of reading actual runtime library
    // paths from the SDK's text-based stub files.
    if let Some(sysroot) = output
        .build_configuration
        .variant
        .get(&"CONDA_BUILD_SYSROOT".into())
    {
        let sysroot_path = PathBuf::from(sysroot.to_string());
        if sysroot_path.exists() {
            for name in extract_tbd_install_names(&sysroot_path) {
                system_libs.add(Glob::new(&name)?);
            }
            return Ok(());
        }
    }

    // Fallback: match conda-build behavior - allow any library in sysroot directories
    // https://github.com/conda/conda-build/blob/61e9bb24588d8b353321c11de5452d57aa2f85ca/conda_build/post.py#L1371-L1384
    const DEFAULT_SYSROOT_PATTERNS: &[&str] = &[
        "/usr/lib/**/*",
        "/opt/X11/**/*.dylib",
        // e.g. /System/Library/Frameworks/AGL.framework/*
        "/System/Library/Frameworks/*.framework/*",
    ];

    for pattern in DEFAULT_SYSROOT_PATTERNS {
        system_libs.add(Glob::new(pattern)?);
    }

    Ok(())
}

fn add_windows_system_libs(
    output: &Output,
    allow_builder: &mut GlobSetBuilder,
    deny_builder: &mut GlobSetBuilder,
) -> Result<(), LinkingCheckError> {
    let subdir = output.build_configuration.target_platform.to_string();

    // Load dsolists from both build and host prefixes
    let (build_allow, build_deny) = load_dsolists(
        &output.build_configuration.directories.build_prefix,
        &subdir,
    )?;
    let (host_allow, host_deny) = load_dsolists(output.prefix(), &subdir)?;

    let mut all_allow: Vec<String> = build_allow.into_iter().chain(host_allow).collect();
    let all_deny: Vec<String> = build_deny.into_iter().chain(host_deny).collect();

    if all_allow.is_empty() && all_deny.is_empty() {
        // No dsolists found: fall back to hardcoded WIN_ALLOWLIST.
        // Use **/ prefix so patterns match both bare names ("KERNEL32.dll")
        // and fully resolved paths ("C:\Windows\system32\KERNEL32.dll").
        for pattern in WIN_ALLOWLIST {
            let full_pattern = format!("**/{pattern}");
            allow_builder.add(GlobBuilder::new(&full_pattern).case_insensitive(true).build()?);
        }
        return Ok(());
    }

    if all_allow.is_empty() && !all_deny.is_empty() {
        // Only deny lists found: default allow is C:/Windows/System32/*.dll
        all_allow.push("C:/Windows/System32/*.dll".to_string());
    }

    let expanded_allow = expand_dsolist(&all_allow);
    let expanded_deny = expand_dsolist(&all_deny);

    for pattern in &expanded_allow {
        allow_builder.add(GlobBuilder::new(pattern).case_insensitive(true).build()?);
    }

    for pattern in &expanded_deny {
        deny_builder.add(GlobBuilder::new(pattern).case_insensitive(true).build()?);
    }

    Ok(())
}

fn add_linux_system_libs(
    output: &Output,
    system_libs: &mut GlobSetBuilder,
) -> Result<(), globset::Error> {
    if let Some(sysroot_package) = output
        .finalized_dependencies
        .clone()
        .expect("failed to get the finalized dependencies")
        .build
        .and_then(|deps| {
            deps.resolved.into_iter().find(|v| {
                v.identifier.identifier.name.starts_with(&format!(
                    "sysroot_{}",
                    output.build_configuration.target_platform
                ))
            })
        })
    {
        let prefix_record_name = format!(
            "conda-meta/{}-{}-{}.json",
            sysroot_package.package_record.name.as_normalized(),
            sysroot_package.package_record.version,
            sysroot_package.package_record.build
        );

        let sysroot_path = output
            .build_configuration
            .directories
            .build_prefix
            .join(prefix_record_name);
        let record = PrefixRecord::from_path(sysroot_path).unwrap();
        let so_glob = Glob::new("*.so*")?.compile_matcher();
        for file in record.files {
            if let Some(file_name) = file.file_name()
                && so_glob.is_match(file_name)
            {
                system_libs.add(Glob::new(&file_name.to_string_lossy())?);
            }
        }
    }
    Ok(())
}

/// System libraries configuration with allow and deny sets.
struct SystemLibs {
    allow: GlobSet,
    deny: GlobSet,
}

/// Returns the system libraries found in sysroot.
fn find_system_libs(output: &Output) -> Result<SystemLibs, LinkingCheckError> {
    let mut allow_builder = GlobSetBuilder::new();
    let mut deny_builder = GlobSetBuilder::new();
    let platform = &output.build_configuration.target_platform;

    if platform.is_osx() {
        add_osx_system_libs(output, &mut allow_builder)?;
    } else if platform.is_windows() {
        add_windows_system_libs(output, &mut allow_builder, &mut deny_builder)?;
    } else {
        add_linux_system_libs(output, &mut allow_builder)?;
    }

    Ok(SystemLibs {
        allow: allow_builder.build()?,
        deny: deny_builder.build()?,
    })
}

pub fn perform_linking_checks(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    tmp_prefix: &Path,
) -> Result<(), LinkingCheckError> {
    let dynamic_linking = &output.recipe.build().dynamic_linking;
    let system_libs = find_system_libs(output)?;

    let prefix_info = PrefixInfo::from_prefix(output.prefix())?;

    let resolved_run_dependencies =
        resolved_run_dependencies(output, &prefix_info.package_to_nature);
    tracing::trace!("Resolved run dependencies: {resolved_run_dependencies:#?}",);

    // check all DSOs and what they are linking
    let target_platform = output.target_platform();
    let host_prefix = output.prefix();

    // Parallel processing of DSO files
    let package_files: Vec<PackageFile> = new_files
        .par_iter()
        .filter_map(|file| {
            // Parse the DSO to get the list of libraries it links to
            match relink::get_relinker(output.build_configuration.target_platform, file) {
                Ok(relinker) => {
                    let mut file_dsos = Vec::new();

                    let resolved_libraries = relinker.resolve_libraries(tmp_prefix, host_prefix);
                    for (lib, resolved) in &resolved_libraries {
                        // filter out @self on macOS
                        if target_platform.is_osx() && lib.to_str() == Some("self") {
                            continue;
                        }

                        let lib = resolved.as_ref().unwrap_or(lib);
                        if let Ok(libpath) = lib.strip_prefix(host_prefix)
                            && let Some(package) = prefix_info
                                .path_to_package
                                .get(&libpath.to_path_buf().into())
                            && let Some(nature) = prefix_info.package_to_nature.get(package)
                        {
                            // Accept any package that provides shared objects (DSO libraries,
                            // interpreters like python providing python3XX.dll, plugin libraries, etc.)
                            if nature.provides_shared_objects() {
                                file_dsos.push((libpath.to_path_buf(), package.clone()));
                            }
                        }
                    }

                    Some(PackageFile {
                        file: file
                            .clone()
                            .strip_prefix(tmp_prefix)
                            .unwrap_or(file)
                            .to_path_buf(),
                        linked_dsos: file_dsos.into_iter().collect(),
                        shared_libraries: resolved_libraries
                            .into_iter()
                            .map(|(v, res)| res.unwrap_or(v.to_path_buf()))
                            .collect(),
                    })
                }
                Err(RelinkError::UnknownFileFormat) => None,
                Err(e) => {
                    tracing::error!("Failed to get relinker for file {}: {}", file.display(), e);
                    None
                }
            }
        })
        .collect();
    tracing::trace!("Package files: {package_files:#?}");

    let mut linked_packages = Vec::new();
    for package in package_files.iter() {
        let mut link_info = PackageLinkInfo {
            file: package.file.clone(),
            linked_packages: Vec::new(),
        };
        // If the package that we are linking against does not exist in run
        // dependencies then it is "overlinking".
        for lib in &package.shared_libraries {
            let lib = lib.strip_prefix(host_prefix).unwrap_or(lib);

            // skip @self on macOS
            if target_platform.is_osx() && lib.to_str() == Some("self") {
                continue;
            }

            // Check if the package has the library linked.
            if let Some(package) = package.linked_dsos.get(lib) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::ForeignPackage(package.as_normalized().to_string()),
                });
                continue;
            }

            // Check if the library is one of the system libraries (i.e. comes from sysroot).
            if system_libs.allow.is_match(lib) && !system_libs.deny.is_match(lib) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::System,
                });
                continue;
            }

            // Check if the package itself has the shared library.
            if new_files.iter().any(|file| file.ends_with(lib)) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::PackageItself,
                });
                continue;
            }

            // Check if we allow overlinking.
            if dynamic_linking.missing_dso_allowlist.is_match(lib) {
                tracing::info!(
                    "{lib:?} is missing in run dependencies for {:?}, \
                    yet it is included in the allow list. Skipping...",
                    package.file
                );
            // Error on overlinking.
            } else if dynamic_linking.overlinking_behavior == LinkingCheckBehavior::Error {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::NotFound,
                });
                linked_packages.push(link_info);
                linked_packages.iter().for_each(|linked_package| {
                    tracing::info!("\n{linked_package}");
                });

                return Err(LinkingCheckError::Overlinking {
                    package: lib.to_path_buf(),
                    file: package.file.clone(),
                });
            } else {
                let warn_str = format!("Overlinking against {lib:?} for {:?}", package.file);
                tracing::warn!(warn_str);
                output.record_warning(&warn_str);
            }

            link_info.linked_packages.push(LinkedPackage {
                name: lib.to_path_buf(),
                link_origin: LinkOrigin::NotFound,
            });
        }
        linked_packages.push(link_info);
    }

    linked_packages.iter().for_each(|linked_package| {
        tracing::info!("{linked_package}");
    });

    // If there are any unused run dependencies then it is "overdepending".
    for run_dependency in resolved_run_dependencies.iter() {
        if !package_files
            .iter()
            .map(|package| {
                package
                    .linked_dsos
                    .values()
                    .map(|v| v.as_source().to_string())
                    .collect::<Vec<String>>()
            })
            .any(|libraries| libraries.contains(run_dependency))
        {
            if dynamic_linking.overdepending_behavior == LinkingCheckBehavior::Error {
                return Err(LinkingCheckError::Overdepending {
                    package: PathBuf::from(run_dependency),
                });
            }
            tracing::warn!("Overdepending against {run_dependency}");
            output.record_warning(&format!("Overdepending against {run_dependency}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_err;

    #[test]
    fn test_extract_tbd_install_names() {
        let test_sysroot = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/tbd_files");

        let install_names = extract_tbd_install_names(&test_sysroot);

        // Should extract install names from both .tbd files
        assert!(
            install_names.contains(&"/usr/lib/libSystem.B.dylib".to_string()),
            "Should contain libSystem.B.dylib, got: {:?}",
            install_names
        );
        assert!(
            install_names.contains(&"/usr/lib/libz.1.dylib".to_string()),
            "Should contain libz.1.dylib, got: {:?}",
            install_names
        );
    }

    #[test]
    fn test_load_dsolists_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["C:/Windows/System32/KERNEL32.dll", "C:/Windows/System32/USER32.dll"],
            "deny": ["C:/Windows/System32/ucrtbased.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let (allow, deny) = load_dsolists(tmp.path(), "win-64").unwrap();
        assert_eq!(
            allow,
            vec![
                "C:/Windows/System32/KERNEL32.dll",
                "C:/Windows/System32/USER32.dll"
            ]
        );
        assert_eq!(deny, vec!["C:/Windows/System32/ucrtbased.dll"]);
    }

    #[test]
    fn test_load_dsolists_wrong_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "linux-64",
            "allow": ["**/libc.so.6"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let (allow, deny) = load_dsolists(tmp.path(), "win-64").unwrap();
        assert!(allow.is_empty());
        assert!(deny.is_empty());
    }

    #[test]
    fn test_load_dsolists_invalid_version_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 99,
            "subdir": "win-64",
            "allow": ["C:/Windows/System32/KERNEL32.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let result = load_dsolists(tmp.path(), "win-64");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported dsolist version 99"), "got: {err}");
    }

    #[test]
    fn test_load_dsolists_no_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (allow, deny) = load_dsolists(tmp.path(), "win-64").unwrap();
        assert!(allow.is_empty());
        assert!(deny.is_empty());
    }

    #[test]
    fn test_load_dsolists_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json1 = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["C:/Windows/System32/KERNEL32.dll"]
        });
        let json2 = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["C:/Windows/System32/USER32.dll"],
            "deny": ["**/ucrtbased.dll"]
        });
        fs_err::write(dsolists_dir.join("a.json"), json1.to_string()).unwrap();
        fs_err::write(dsolists_dir.join("b.json"), json2.to_string()).unwrap();

        let (allow, deny) = load_dsolists(tmp.path(), "win-64").unwrap();
        assert!(allow.contains(&"C:/Windows/System32/KERNEL32.dll".to_string()));
        assert!(allow.contains(&"C:/Windows/System32/USER32.dll".to_string()));
        assert_eq!(deny, vec!["**/ucrtbased.dll"]);
    }

    #[test]
    fn test_load_dsolists_rejects_backslashes() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["C:\\Windows\\System32\\KERNEL32.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let result = load_dsolists(tmp.path(), "win-64");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("backslashes"), "got: {err}");
    }

    #[test]
    fn test_load_dsolists_rejects_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["relative/path/KERNEL32.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let result = load_dsolists(tmp.path(), "win-64");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not an absolute path"), "got: {err}");
    }

    #[test]
    fn test_load_dsolists_accepts_glob_star_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["**/R.dll", "C:/Windows/System32/*.dll"],
            "deny": ["**/ucrtbased.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let (allow, deny) = load_dsolists(tmp.path(), "win-64").unwrap();
        assert_eq!(allow, vec!["**/R.dll", "C:/Windows/System32/*.dll"]);
        assert_eq!(deny, vec!["**/ucrtbased.dll"]);
    }

    #[test]
    fn test_validate_dsolist_pattern() {
        let file = Path::new("test.json");

        // Valid patterns
        assert!(validate_dsolist_pattern("C:/Windows/System32/*.dll", file).is_ok());
        assert!(validate_dsolist_pattern("D:/some/path/lib.dll", file).is_ok());
        assert!(validate_dsolist_pattern("**/R.dll", file).is_ok());

        // Invalid: backslashes
        assert!(validate_dsolist_pattern("C:\\Windows\\System32\\foo.dll", file).is_err());

        // Invalid: Unix absolute paths
        assert!(validate_dsolist_pattern("/usr/lib/libc.so.6", file).is_err());

        // Invalid: relative path
        assert!(validate_dsolist_pattern("relative/foo.dll", file).is_err());
        assert!(validate_dsolist_pattern("foo.dll", file).is_err());

        // Invalid: bare glob without **
        assert!(validate_dsolist_pattern("*.dll", file).is_err());
    }

    #[test]
    fn test_expand_dsolist_wildcard_passthrough() {
        let patterns = vec!["*.dll".to_string(), "**/foo.dll".to_string()];
        let expanded = expand_dsolist(&patterns);
        assert_eq!(expanded, vec!["*.dll", "**/foo.dll"]);
    }

    #[test]
    fn test_expand_dsolist_absolute_path_conversion() {
        let patterns = vec![
            "C:/Windows/System32/KERNEL32.dll".to_string(),
            "/usr/lib/libc.so.6".to_string(),
        ];
        let expanded = expand_dsolist(&patterns);
        assert_eq!(expanded, vec!["**/KERNEL32.dll", "**/libc.so.6"]);
    }

    #[test]
    fn test_expand_dsolist_mixed() {
        let patterns = vec![
            "*.dll".to_string(),
            "C:/Windows/System32/ucrtbased.dll".to_string(),
        ];
        let expanded = expand_dsolist(&patterns);
        assert_eq!(expanded, vec!["*.dll", "**/ucrtbased.dll"]);
    }

    #[test]
    fn test_validate_dsolist_files_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["C:/Windows/System32/*.dll", "**/R.dll"],
            "deny": ["**/ucrtbased.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        assert!(validate_dsolist_files(tmp.path()).is_ok());
    }

    #[test]
    fn test_validate_dsolist_files_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(validate_dsolist_files(tmp.path()).is_ok());
    }

    #[test]
    fn test_validate_dsolist_files_rejects_invalid_version() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 2,
            "subdir": "win-64",
            "allow": ["C:/Windows/System32/*.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let result = validate_dsolist_files(tmp.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported dsolist version 2")
        );
    }

    #[test]
    fn test_validate_dsolist_files_rejects_backslashes() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["C:\\Windows\\System32\\foo.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let result = validate_dsolist_files(tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("backslashes"));
    }

    #[test]
    fn test_validate_dsolist_files_rejects_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "subdir": "win-64",
            "allow": ["relative/KERNEL32.dll"]
        });
        fs_err::write(dsolists_dir.join("test.json"), json.to_string()).unwrap();

        let result = validate_dsolist_files(tmp.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not an absolute path")
        );
    }

    #[test]
    fn test_validate_dsolist_files_rejects_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let dsolists_dir = tmp.path().join("etc/conda-build/dsolists.d");
        fs_err::create_dir_all(&dsolists_dir).unwrap();

        fs_err::write(dsolists_dir.join("bad.json"), "not valid json").unwrap();

        let result = validate_dsolist_files(tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }
}
