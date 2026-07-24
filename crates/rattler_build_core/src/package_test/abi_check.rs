//! Implementation of the `abi_check` package test.
//!
//! The test downloads the lowest previously published version of the package that matches
//! the configured pin expression (e.g. `x.x`, as used in pinnings) and compares the ABI
//! surface of the shared libraries shipped in both packages. Within the pinned version
//! range the following are reported as ABI breaks:
//!
//! * a shared library that disappeared from the package
//! * a changed soname (ELF), install name (Mach-O) or DLL name (PE)
//! * exported symbols that were removed
//!
//! Note that this is a *surface* check based on the dynamic symbol tables / export
//! tables (parsed with `goblin`): changes to function signatures or struct layouts that
//! do not change the set of exported symbol names cannot be detected.

use std::{
    cmp,
    collections::BTreeSet,
    path::{Path, PathBuf},
    str::FromStr,
};

use fs_err as fs;
use goblin::{
    Object,
    elf::Elf,
    mach::{Mach, MachO, SingleArch},
};
use rattler::package_cache::CacheKey;
use rattler_build_recipe::stage1::{GlobVec, tests::AbiCheckTest};
use rattler_conda_types::{
    Channel, MatchSpec, PackageName, PackageNameMatcher, Platform, RepoDataRecord, Version,
    package::CondaArchiveIdentifier,
};

use super::run_test::{TestConfiguration, TestError};

/// The ABI surface of a single shared library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAbi {
    /// Path of the library relative to the package root
    pub relative_path: PathBuf,
    /// The soname (ELF), install name (Mach-O) or DLL name (PE) of the library, if any
    pub soname: Option<String>,
    /// All exported (defined, externally visible) symbol names
    pub exported_symbols: BTreeSet<String>,
    /// The number of exports without a name (e.g. ordinal-only exports on Windows)
    pub unnamed_exports: usize,
}

impl LibraryAbi {
    /// The name used to pair a library with its counterpart in the other package when the
    /// relative path does not match exactly (e.g. `libfoo.so.1.2.3` -> `libfoo.so`).
    fn normalized_name(&self) -> String {
        let file_name = self
            .relative_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        normalized_library_name(&file_name)
    }
}

/// Strip version suffixes from a shared library file name so that differently versioned
/// builds of the same library can be paired with each other.
fn normalized_library_name(file_name: &str) -> String {
    // ELF: `libfoo.so.1.2.3` -> `libfoo.so`
    if file_name.ends_with(".so") {
        return file_name.to_string();
    }
    if let Some(idx) = file_name.find(".so.") {
        return file_name[..idx + 3].to_string();
    }
    // Mach-O: `libfoo.1.2.dylib` -> `libfoo.dylib`
    if let Some(stem) = file_name.strip_suffix(".dylib") {
        let base = stem
            .split('.')
            .take_while(|part| !part.chars().all(|c| c.is_ascii_digit()))
            .collect::<Vec<_>>()
            .join(".");
        if !base.is_empty() {
            return format!("{base}.dylib");
        }
        return file_name.to_string();
    }
    // PE (and everything else): DLL names are case-insensitive
    file_name.to_lowercase()
}

/// Extract the ABI surface from a binary, or `None` if the file is not a shared library.
fn extract_library_abi(bytes: &[u8], relative_path: &Path) -> Option<LibraryAbi> {
    match Object::parse(bytes).ok()? {
        Object::Elf(elf) => elf_abi(&elf, relative_path),
        Object::Mach(Mach::Binary(macho)) => macho_abi(&macho, relative_path),
        Object::Mach(Mach::Fat(multi_arch)) => {
            // For fat binaries take the union of the exported symbols of all slices
            let mut combined: Option<LibraryAbi> = None;
            for arch in &multi_arch {
                if let Ok(SingleArch::MachO(macho)) = arch
                    && let Some(abi) = macho_abi(&macho, relative_path)
                {
                    match combined.as_mut() {
                        None => combined = Some(abi),
                        Some(existing) => {
                            existing.exported_symbols.extend(abi.exported_symbols);
                        }
                    }
                }
            }
            combined
        }
        Object::PE(pe) => {
            if !pe.is_lib {
                return None;
            }
            let mut exported_symbols = BTreeSet::new();
            let mut unnamed_exports = 0;
            for export in &pe.exports {
                match export.name {
                    Some(name) if !name.is_empty() => {
                        exported_symbols.insert(name.to_string());
                    }
                    _ => unnamed_exports += 1,
                }
            }
            Some(LibraryAbi {
                relative_path: relative_path.to_path_buf(),
                soname: pe.name.map(str::to_lowercase),
                exported_symbols,
                unnamed_exports,
            })
        }
        _ => None,
    }
}

fn elf_abi(elf: &Elf<'_>, relative_path: &Path) -> Option<LibraryAbi> {
    use goblin::elf::{
        header::ET_DYN,
        section_header::SHN_UNDEF,
        sym::{STB_GLOBAL, STB_WEAK},
    };

    // Only shared objects (this includes Python extension modules); skip executables,
    // including position independent ones (which are ET_DYN but have an interpreter).
    if elf.header.e_type != ET_DYN || elf.interpreter.is_some() {
        return None;
    }

    let mut exported_symbols = BTreeSet::new();
    for sym in elf.dynsyms.iter() {
        // Skip undefined symbols (these are imports, not exports)
        if sym.st_shndx == SHN_UNDEF as usize {
            continue;
        }
        let bind = sym.st_bind();
        if bind != STB_GLOBAL && bind != STB_WEAK {
            continue;
        }
        if let Some(name) = elf.dynstrtab.get_at(sym.st_name)
            && !name.is_empty()
        {
            exported_symbols.insert(name.to_string());
        }
    }

    Some(LibraryAbi {
        relative_path: relative_path.to_path_buf(),
        soname: elf.soname.map(str::to_string),
        exported_symbols,
        unnamed_exports: 0,
    })
}

fn macho_abi(macho: &MachO<'_>, relative_path: &Path) -> Option<LibraryAbi> {
    use goblin::mach::header::{MH_BUNDLE, MH_DYLIB};

    // Only dylibs and loadable bundles (Python extension modules)
    if !matches!(macho.header.filetype, MH_DYLIB | MH_BUNDLE) {
        return None;
    }

    let mut exported_symbols = BTreeSet::new();

    // The export trie is the authoritative source for exported symbols
    if let Ok(exports) = macho.exports() {
        for export in exports {
            if !export.name.is_empty() {
                exported_symbols.insert(export.name);
            }
        }
    }

    // Also collect external defined symbols from the symbol table as a fallback for
    // binaries without (or with an empty) export trie
    if exported_symbols.is_empty() {
        for symbol in macho.symbols() {
            if let Ok((name, nlist)) = symbol
                && nlist.is_global()
                && !nlist.is_undefined()
                && !nlist.is_stab()
                && !name.is_empty()
            {
                exported_symbols.insert(name.to_string());
            }
        }
    }

    // Use the file name portion of the install name as the "soname": the directory part
    // (e.g. `@rpath/` or an encoded prefix) is not relevant for ABI compatibility
    let soname = macho.name.map(|id| {
        Path::new(id)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| id.to_string())
    });

    Some(LibraryAbi {
        relative_path: relative_path.to_path_buf(),
        soname,
        exported_symbols,
        unnamed_exports: 0,
    })
}

/// Collect the ABI surface of all shared libraries in an extracted package directory.
///
/// If `libraries` is not empty, only libraries whose relative path matches the globs are
/// collected.
fn collect_libraries(
    package_dir: &Path,
    libraries: &GlobVec,
) -> Result<Vec<LibraryAbi>, TestError> {
    let mut result = Vec::new();

    for entry in walkdir::WalkDir::new(package_dir)
        .into_iter()
        .filter_entry(|entry| {
            // Skip the `info/` metadata directory at the package root
            entry.path().strip_prefix(package_dir) != Ok(Path::new("info"))
        })
    {
        let entry = entry.map_err(|e| {
            TestError::AbiCheckError(format!("failed to walk package directory: {e}"))
        })?;
        // Skip directories and symlinks (the symlink targets are checked instead)
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry
            .path()
            .strip_prefix(package_dir)
            .expect("entry must be under the package directory")
            .to_path_buf();

        if !libraries.is_empty() && !libraries.is_match(&relative_path) {
            continue;
        }

        let bytes = fs::read(entry.path())?;
        if let Some(abi) = extract_library_abi(&bytes, &relative_path) {
            result.push(abi);
        }
    }

    result.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(result)
}

/// A single detected ABI incompatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AbiViolation {
    /// A shared library from the baseline package has no counterpart in the new package
    LibraryRemoved {
        path: PathBuf,
        soname: Option<String>,
    },
    /// The soname / install name / DLL name of a library changed
    SonameChanged {
        path: PathBuf,
        old: String,
        new: String,
    },
    /// Exported symbols were removed from a library
    SymbolsRemoved {
        path: PathBuf,
        matched_path: PathBuf,
        symbols: Vec<String>,
    },
    /// The number of unnamed (ordinal-only) exports decreased (PE only)
    UnnamedExportsRemoved {
        path: PathBuf,
        old_count: usize,
        new_count: usize,
    },
}

impl std::fmt::Display for AbiViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        /// Number of removed symbols listed per library before truncating the output
        const MAX_LISTED_SYMBOLS: usize = 25;

        match self {
            AbiViolation::LibraryRemoved { path, soname } => {
                write!(f, "{}: shared library was removed", path.display())?;
                if let Some(soname) = soname {
                    write!(f, " (soname `{soname}`)")?;
                }
                Ok(())
            }
            AbiViolation::SonameChanged { path, old, new } => {
                write!(
                    f,
                    "{}: soname changed from `{old}` to `{new}`",
                    path.display()
                )
            }
            AbiViolation::SymbolsRemoved {
                path,
                matched_path,
                symbols,
            } => {
                write!(
                    f,
                    "{}: {} exported symbol{} removed",
                    path.display(),
                    symbols.len(),
                    if symbols.len() == 1 { "" } else { "s" },
                )?;
                if path != matched_path {
                    write!(f, " (compared with {})", matched_path.display())?;
                }
                for symbol in symbols.iter().take(MAX_LISTED_SYMBOLS) {
                    write!(f, "\n    - {symbol}")?;
                }
                if symbols.len() > MAX_LISTED_SYMBOLS {
                    write!(
                        f,
                        "\n    ... and {} more",
                        symbols.len() - MAX_LISTED_SYMBOLS
                    )?;
                }
                Ok(())
            }
            AbiViolation::UnnamedExportsRemoved {
                path,
                old_count,
                new_count,
            } => {
                write!(
                    f,
                    "{}: the number of unnamed (ordinal-only) exports decreased from {old_count} to {new_count}",
                    path.display()
                )
            }
        }
    }
}

/// Find the counterpart of `old` in `new_libs`. Match by (in order of preference) exact
/// relative path, normalized file name in the same directory, soname, and normalized file
/// name anywhere in the package.
fn find_matching_library<'a>(
    old: &LibraryAbi,
    new_libs: &'a [LibraryAbi],
) -> Option<&'a LibraryAbi> {
    if let Some(exact) = new_libs
        .iter()
        .find(|lib| lib.relative_path == old.relative_path)
    {
        return Some(exact);
    }

    let normalized = old.normalized_name();
    if let Some(same_dir) = new_libs.iter().find(|lib| {
        lib.relative_path.parent() == old.relative_path.parent()
            && lib.normalized_name() == normalized
    }) {
        return Some(same_dir);
    }

    if old.soname.is_some()
        && let Some(same_soname) = new_libs.iter().find(|lib| lib.soname == old.soname)
    {
        return Some(same_soname);
    }

    new_libs
        .iter()
        .find(|lib| lib.normalized_name() == normalized)
}

/// Compare the ABI surfaces of the baseline (old) and the new package.
pub(crate) fn diff_library_abis(
    old_libs: &[LibraryAbi],
    new_libs: &[LibraryAbi],
    ignore_symbols: &GlobVec,
) -> Vec<AbiViolation> {
    let mut violations = Vec::new();

    for old in old_libs {
        let Some(new) = find_matching_library(old, new_libs) else {
            violations.push(AbiViolation::LibraryRemoved {
                path: old.relative_path.clone(),
                soname: old.soname.clone(),
            });
            continue;
        };

        if let (Some(old_soname), Some(new_soname)) = (&old.soname, &new.soname)
            && old_soname != new_soname
        {
            violations.push(AbiViolation::SonameChanged {
                path: old.relative_path.clone(),
                old: old_soname.clone(),
                new: new_soname.clone(),
            });
        }

        let removed: Vec<String> = old
            .exported_symbols
            .difference(&new.exported_symbols)
            .filter(|symbol| {
                ignore_symbols.is_empty() || !ignore_symbols.is_match(Path::new(symbol))
            })
            .cloned()
            .collect();
        if !removed.is_empty() {
            violations.push(AbiViolation::SymbolsRemoved {
                path: old.relative_path.clone(),
                matched_path: new.relative_path.clone(),
                symbols: removed,
            });
        }

        if old.unnamed_exports > new.unnamed_exports {
            violations.push(AbiViolation::UnnamedExportsRemoved {
                path: old.relative_path.clone(),
                old_count: old.unnamed_exports,
                new_count: new.unnamed_exports,
            });
        }
    }

    violations
}

/// The length of the common prefix of two build strings. Used to prefer the baseline
/// build variant that is closest to the current one (e.g. `py310h1234_0` over
/// `py39h5678_0` when the current build string is `py310habcd_1`).
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// From all records matching the pinned version range, select the baseline record: the
/// lowest version, preferring the build variant closest to the current build string.
fn select_baseline_record<'a>(
    candidates: &[&'a RepoDataRecord],
    current_build_string: &str,
) -> Option<&'a RepoDataRecord> {
    let lowest_version = candidates
        .iter()
        .map(|record| record.package_record.version.clone())
        .min()?;

    candidates
        .iter()
        .filter(|record| record.package_record.version == lowest_version)
        .max_by_key(|record| {
            (
                common_prefix_len(&record.package_record.build, current_build_string),
                record.package_record.build_number,
            )
        })
        .copied()
}

/// Execute the ABI check test: download the baseline package and compare the ABI surface
/// of its shared libraries with the package under test.
pub(crate) async fn run_abi_check_test(
    abi_check: &AbiCheckTest,
    pkg: &CondaArchiveIdentifier,
    package_folder: &Path,
    config: &TestConfiguration,
) -> Result<(), TestError> {
    let pkg_id = format!(
        "{}-{}-{}",
        pkg.identifier.name, pkg.identifier.version, pkg.identifier.build_string
    );
    let span = tracing::info_span!("Running ABI check", span_color = pkg_id);
    let _guard = span.enter();

    let target_platform = config
        .target_platform
        .unwrap_or(config.current_platform.platform);
    if target_platform == Platform::NoArch {
        tracing::info!("Skipping ABI check for noarch package");
        return Ok(());
    }

    let package_name = PackageName::try_from(pkg.identifier.name.as_str())
        .map_err(|e| TestError::AbiCheckError(format!("invalid package name: {e}")))?;
    let current_version = Version::from_str(&pkg.identifier.version)
        .map_err(|e| TestError::AbiCheckError(format!("invalid package version: {e}")))?;

    // Compute the lower bound of the pinned version range from the pin expression, e.g.
    // pin `x.x` and version `1.2.3` -> lower bound `1.2` (mirroring `Pin::apply`)
    let pin_digits = abi_check
        .pin
        .to_string()
        .chars()
        .filter(|&c| c == 'x')
        .count();
    if pin_digits == 0 {
        return Err(TestError::AbiCheckError(
            "the pin expression must contain at least one `x`".to_string(),
        ));
    }
    let lower_bound = current_version
        .with_segments(..cmp::min(pin_digits, current_version.segment_count()))
        .ok_or_else(|| {
            TestError::AbiCheckError(format!(
                "could not apply pin `{}` to version `{current_version}`",
                abi_check.pin
            ))
        })?;

    tracing::info!(
        "Looking for the lowest published version of `{}` matching `>={lower_bound},<{current_version}` (pin: `{}`)",
        package_name.as_normalized(),
        abi_check.pin,
    );

    // Query the repodata of all test channels for the package
    let channels = config
        .channels
        .iter()
        .map(|url| Channel::from_url(url.clone()))
        .collect::<Vec<_>>();
    let name_only_spec = MatchSpec {
        name: PackageNameMatcher::Exact(package_name.clone()),
        ..Default::default()
    };
    let repodata = config
        .tool_configuration
        .repodata_gateway
        .query(channels, [target_platform], vec![name_only_spec])
        .await
        .map_err(|e| TestError::AbiCheckError(format!("failed to query repodata: {e}")))?;

    let candidates: Vec<&RepoDataRecord> = repodata
        .iter()
        .flat_map(|repo_data| repo_data.iter())
        .filter(|record| record.package_record.name == package_name)
        .filter(|record| {
            let version = record.package_record.version.version();
            version >= &lower_bound && version < &current_version
        })
        .filter(|record| {
            // Respect `--exclude-newer` if configured
            match (config.exclude_newer, record.package_record.timestamp) {
                (Some(exclude_newer), Some(timestamp)) => {
                    timestamp.timestamp_millis() <= exclude_newer.as_millisecond()
                }
                _ => true,
            }
        })
        .collect();

    let Some(baseline) = select_baseline_record(&candidates, &pkg.identifier.build_string) else {
        tracing::warn!(
            "No previously published version of `{}` matches `>={lower_bound},<{current_version}` — skipping ABI check (this is expected for the first release in a pin range)",
            package_name.as_normalized(),
        );
        return Ok(());
    };

    tracing::info!(
        "Comparing ABI with baseline {}={}={}",
        baseline.package_record.name.as_normalized(),
        baseline.package_record.version,
        baseline.package_record.build,
    );

    // Download (or copy) and extract the baseline package via the package cache
    let package_cache = &config.tool_configuration.package_cache;
    let cache_metadata = if baseline.url.scheme() == "file" {
        let path = baseline
            .url
            .to_file_path()
            .map_err(|_| TestError::AbiCheckError(format!("invalid file URL: {}", baseline.url)))?;
        package_cache
            .get_or_fetch_from_path(&path, Some(&baseline.package_record), None)
            .await
    } else {
        package_cache
            .get_or_fetch_from_url(
                CacheKey::from(&baseline.package_record),
                baseline.url.clone(),
                config
                    .tool_configuration
                    .client
                    .for_host(&baseline.url)
                    .clone(),
                None,
                None,
            )
            .await
    }
    .map_err(|e| TestError::AbiCheckError(format!("failed to fetch the baseline package: {e}")))?;
    let baseline_folder = cache_metadata.path().to_path_buf();

    let old_libs = collect_libraries(&baseline_folder, &abi_check.libraries)?;
    let new_libs = collect_libraries(package_folder, &GlobVec::default())?;

    if old_libs.is_empty() {
        tracing::warn!(
            "The baseline package does not contain any shared libraries{} — nothing to check",
            if abi_check.libraries.is_empty() {
                String::new()
            } else {
                format!(" matching {:?}", abi_check.libraries)
            }
        );
        return Ok(());
    }

    let violations = diff_library_abis(&old_libs, &new_libs, &abi_check.ignore_symbols);

    if violations.is_empty() {
        let symbol_count: usize = old_libs.iter().map(|lib| lib.exported_symbols.len()).sum();
        tracing::info!(
            "{} ABI check passed: {} librar{} ({} exported symbols) compatible with {}={}={}",
            console::style(console::Emoji("✔", "")).green(),
            old_libs.len(),
            if old_libs.len() == 1 { "y" } else { "ies" },
            symbol_count,
            baseline.package_record.name.as_normalized(),
            baseline.package_record.version,
            baseline.package_record.build,
        );
        Ok(())
    } else {
        let mut report = format!(
            "found {} ABI incompatibilit{} compared to {}={}={}:",
            violations.len(),
            if violations.len() == 1 { "y" } else { "ies" },
            baseline.package_record.name.as_normalized(),
            baseline.package_record.version,
            baseline.package_record.build,
        );
        for violation in &violations {
            report.push_str("\n  ✖ ");
            report.push_str(&violation.to_string());
        }
        tracing::error!("{report}");
        Err(TestError::AbiCheckFailed(report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(path: &str, soname: Option<&str>, symbols: &[&str]) -> LibraryAbi {
        LibraryAbi {
            relative_path: PathBuf::from(path),
            soname: soname.map(str::to_string),
            exported_symbols: symbols.iter().map(|s| s.to_string()).collect(),
            unnamed_exports: 0,
        }
    }

    #[test]
    fn test_normalized_library_name() {
        assert_eq!(normalized_library_name("libfoo.so"), "libfoo.so");
        assert_eq!(normalized_library_name("libfoo.so.1.2.3"), "libfoo.so");
        assert_eq!(normalized_library_name("libabsl.so.2308.0.0"), "libabsl.so");
        assert_eq!(normalized_library_name("libfoo.dylib"), "libfoo.dylib");
        assert_eq!(normalized_library_name("libfoo.1.2.dylib"), "libfoo.dylib");
        assert_eq!(
            normalized_library_name("libfoo.bar.1.dylib"),
            "libfoo.bar.dylib"
        );
        assert_eq!(normalized_library_name("Foo.DLL"), "foo.dll");
        assert_eq!(
            normalized_library_name("module.cpython-310-x86_64-linux-gnu.so"),
            "module.cpython-310-x86_64-linux-gnu.so"
        );
    }

    #[test]
    fn test_diff_no_changes() {
        let old = vec![lib("lib/libfoo.so.1", Some("libfoo.so.1"), &["a", "b"])];
        let new = vec![lib(
            "lib/libfoo.so.1",
            Some("libfoo.so.1"),
            &["a", "b", "c"],
        )];
        assert!(diff_library_abis(&old, &new, &GlobVec::default()).is_empty());
    }

    #[test]
    fn test_diff_removed_symbols() {
        let old = vec![lib(
            "lib/libfoo.so.1",
            Some("libfoo.so.1"),
            &["a", "b", "c"],
        )];
        let new = vec![lib("lib/libfoo.so.1", Some("libfoo.so.1"), &["a"])];
        let violations = diff_library_abis(&old, &new, &GlobVec::default());
        assert_eq!(violations.len(), 1);
        match &violations[0] {
            AbiViolation::SymbolsRemoved { symbols, .. } => {
                assert_eq!(symbols, &["b".to_string(), "c".to_string()]);
            }
            other => panic!("expected SymbolsRemoved, got {other:?}"),
        }
    }

    #[test]
    fn test_diff_ignored_symbols() {
        let old = vec![lib(
            "lib/libfoo.so.1",
            Some("libfoo.so.1"),
            &["a", "_internal_x", "_internal_y"],
        )];
        let new = vec![lib("lib/libfoo.so.1", Some("libfoo.so.1"), &["a"])];
        let ignore = GlobVec::from_vec(vec!["_internal_*"], None);
        assert!(diff_library_abis(&old, &new, &ignore).is_empty());
    }

    #[test]
    fn test_diff_version_bumped_file_name() {
        // The file name changed with the version, but the soname stayed stable
        let old = vec![lib("lib/libfoo.so.1.2.0", Some("libfoo.so.1"), &["a"])];
        let new = vec![lib("lib/libfoo.so.1.2.5", Some("libfoo.so.1"), &["a", "b"])];
        assert!(diff_library_abis(&old, &new, &GlobVec::default()).is_empty());
    }

    #[test]
    fn test_diff_soname_changed() {
        let old = vec![lib("lib/libfoo.so.1.0.0", Some("libfoo.so.1"), &["a"])];
        let new = vec![lib("lib/libfoo.so.2.0.0", Some("libfoo.so.2"), &["a"])];
        let violations = diff_library_abis(&old, &new, &GlobVec::default());
        assert_eq!(violations.len(), 1);
        assert!(
            matches!(&violations[0], AbiViolation::SonameChanged { old, new, .. }
            if old == "libfoo.so.1" && new == "libfoo.so.2")
        );
    }

    #[test]
    fn test_diff_library_removed() {
        let old = vec![
            lib("lib/libfoo.so.1", Some("libfoo.so.1"), &["a"]),
            lib("lib/libbar.so.1", Some("libbar.so.1"), &["b"]),
        ];
        let new = vec![lib("lib/libfoo.so.1", Some("libfoo.so.1"), &["a"])];
        let violations = diff_library_abis(&old, &new, &GlobVec::default());
        assert_eq!(violations.len(), 1);
        assert!(
            matches!(&violations[0], AbiViolation::LibraryRemoved { path, .. }
            if path == Path::new("lib/libbar.so.1"))
        );
    }

    #[test]
    fn test_diff_unnamed_exports() {
        let mut old_lib = lib("bin/foo.dll", Some("foo.dll"), &["a"]);
        old_lib.unnamed_exports = 3;
        let mut new_lib = lib("bin/foo.dll", Some("foo.dll"), &["a"]);
        new_lib.unnamed_exports = 1;
        let violations = diff_library_abis(&[old_lib], &[new_lib], &GlobVec::default());
        assert_eq!(violations.len(), 1);
        assert!(matches!(
            &violations[0],
            AbiViolation::UnnamedExportsRemoved {
                old_count: 3,
                new_count: 1,
                ..
            }
        ));
    }

    #[test]
    fn test_common_prefix_len() {
        assert_eq!(common_prefix_len("py310h1234_0", "py310h5678_1"), 6);
        assert_eq!(common_prefix_len("py39h1_0", "py310h1_0"), 3);
        assert_eq!(common_prefix_len("abc", "abc"), 3);
        assert_eq!(common_prefix_len("", "abc"), 0);
    }
}
