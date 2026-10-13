//! `rattler-build package create`: build a conda package from a JSON manifest.
//!
//! This is meant for build systems that already produced the files (Bazel,
//! Buck, Nix, ...) and only need them turned into a correct conda package:
//! the manifest lists the files and the package metadata, and rattler-build
//! takes care of staging, relinking binaries to be relocatable, writing the
//! `info/` metadata and archiving.
//!
//! ```json
//! {
//!   "name": "zversion",
//!   "version": "0.1.0",
//!   "build_string": "h1234abcd_0",
//!   "target_platform": "linux-64",
//!   "depends": ["libzlib >=1.3.2,<2.0a0"],
//!   "files": [{"src": "bazel-out/k8-fastbuild/bin/zversion", "dest": "bin/zversion", "executable": true}],
//!   "symlinks": [{"dest": "bin/zv", "target": "zversion"}]
//! }
//! ```

use std::path::{Component, Path, PathBuf};

use clap::Parser;
use fs_err as fs;
use miette::{Context, IntoDiagnostic};
use rattler_build_core::{RelinkError, get_relinker, system_tools::SystemTools};
use rattler_build_package::{
    AboutJsonBuilder, ArchiveType, IndexJsonBuilder, PackageBuilder, PackageConfig,
};
use rattler_build_types::GlobVec;
use rattler_conda_types::{
    NoArchType, PackageName, Subdir, VersionWithSource, package::RunExportsJson,
};
use serde::Deserialize;

/// Options for `rattler-build package create`.
#[derive(Parser, Debug, Clone)]
pub struct CreateOpts {
    /// The JSON manifest describing the package.
    #[arg(long)]
    pub manifest: PathBuf,

    /// Directory to write the package to. The file name is
    /// `<name>-<version>-<build_string>.conda`.
    #[arg(long, default_value = ".")]
    pub output_dir: PathBuf,
}

/// The package manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Build string, e.g. `h1234abcd_0`.
    pub build_string: String,
    /// Build number.
    #[serde(default)]
    pub build_number: u64,
    /// Target platform (subdir), e.g. `linux-64`; `noarch` for noarch packages.
    pub target_platform: Subdir,
    /// `generic` or `python` for noarch packages.
    #[serde(default)]
    pub noarch: Option<NoArchType>,
    /// Run requirements.
    #[serde(default)]
    pub depends: Vec<String>,
    /// Run constraints.
    #[serde(default)]
    pub constrains: Vec<String>,
    /// Track features.
    #[serde(default)]
    pub track_features: Vec<String>,
    /// The package's own run exports.
    #[serde(default)]
    pub run_exports: Option<RunExportsJson>,
    /// SPDX license expression.
    #[serde(default)]
    pub license: Option<String>,
    /// License family.
    #[serde(default)]
    pub license_family: Option<String>,
    /// One line summary.
    #[serde(default)]
    pub summary: Option<String>,
    /// Long description.
    #[serde(default)]
    pub description: Option<String>,
    /// Homepage URL.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Source repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Documentation URL.
    #[serde(default)]
    pub documentation: Option<String>,
    /// License files, copied to `info/licenses/`.
    #[serde(default)]
    pub license_files: Vec<PathBuf>,
    /// Files to include.
    #[serde(default)]
    pub files: Vec<ManifestFile>,
    /// Symlinks to create.
    #[serde(default)]
    pub symlinks: Vec<ManifestSymlink>,
    /// Timestamp in milliseconds since the epoch, for reproducible archives.
    #[serde(default)]
    pub timestamp: Option<i64>,
    /// Relink ELF / Mach-O files so they find libraries in the environment.
    #[serde(default = "default_true")]
    pub relink: bool,
    /// Rpaths (relative to the prefix) added while relinking.
    #[serde(default = "default_rpaths")]
    pub rpaths: Vec<String>,
    /// Rpath entries containing any of these substrings point into the
    /// build system's tree (e.g. Bazel's `_solib_` / `.runfiles/`). They are
    /// replaced with `rpaths` before relinking.
    #[serde(default)]
    pub strip_rpath_patterns: Vec<String>,
    /// Detect and record prefix placeholders. Off by default: files produced
    /// outside of a conda build prefix don't contain one.
    #[serde(default)]
    pub detect_prefix: bool,
    /// zstd compression level.
    #[serde(default = "default_compression_level")]
    pub compression_level: u8,
    /// Number of compression threads; 1 gives byte-identical archives.
    #[serde(default = "default_threads")]
    pub compression_threads: usize,
}

/// A file in the manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    /// Path on disk (relative to the current directory or absolute).
    pub src: PathBuf,
    /// Path inside the package.
    pub dest: String,
    /// Mark the file executable.
    #[serde(default)]
    pub executable: bool,
}

/// A symlink in the manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSymlink {
    /// Path of the link inside the package.
    pub dest: String,
    /// Link target.
    pub target: String,
}

fn default_true() -> bool {
    true
}

fn default_rpaths() -> Vec<String> {
    vec!["lib/".to_string()]
}

fn default_compression_level() -> u8 {
    15
}

fn default_threads() -> usize {
    1
}

/// Reject absolute paths, `..` and the reserved `info/` directory.
fn check_dest(dest: &str) -> miette::Result<()> {
    let path = Path::new(dest);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        miette::bail!("invalid destination path in package: {dest:?}");
    }
    if path.starts_with("info") {
        miette::bail!("{dest:?}: the info/ directory is reserved for package metadata");
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(path: &Path, _executable: bool) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(path, perms)
}

/// Stage the manifest's files and symlinks into `root`.
fn stage(manifest: &PackageManifest, root: &Path) -> miette::Result<Vec<PathBuf>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut staged = Vec::new();
    for file in &manifest.files {
        check_dest(&file.dest)?;
        if !seen.insert(file.dest.clone()) {
            miette::bail!("two entries map to {:?}", file.dest);
        }
        let target = root.join(&file.dest);
        fs::create_dir_all(target.parent().expect("dest has a parent")).into_diagnostic()?;
        // Copy (follows symlinks): build system outputs are often read-only
        // links into a cache.
        fs::copy(&file.src, &target)
            .into_diagnostic()
            .with_context(|| format!("copying {}", file.src.display()))?;
        set_mode(&target, file.executable).into_diagnostic()?;
        staged.push(target);
    }
    for link in &manifest.symlinks {
        check_dest(&link.dest)?;
        if !seen.insert(link.dest.clone()) {
            miette::bail!("two entries map to {:?}", link.dest);
        }
        let path = root.join(&link.dest);
        fs::create_dir_all(path.parent().expect("dest has a parent")).into_diagnostic()?;
        #[cfg(unix)]
        fs_err::os::unix::fs::symlink(&link.target, &path).into_diagnostic()?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&link.target, &path).into_diagnostic()?;
    }
    Ok(staged)
}

/// ELF files without DT_RPATH / DT_RUNPATH don't reference the build tree,
/// so there is nothing to fix (and adding an rpath would need patchelf).
fn elf_without_rpath(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    match goblin::elf::Elf::parse(&bytes) {
        Ok(elf) => elf.rpaths.is_empty() && elf.runpaths.is_empty(),
        Err(_) => false,
    }
}

/// Replace rpath entries matching `patterns` with the (relocated) `rpaths`,
/// in place. Build systems like Bazel produce long `$ORIGIN`-relative rpaths
/// into their own trees; they may resolve "inside" the staging prefix, so
/// the relinker would keep them and could run out of space for new entries.
fn strip_build_rpaths(
    path: &Path,
    root: &Path,
    patterns: &[String],
    rpaths: &[String],
) -> miette::Result<()> {
    use goblin::elf::dynamic::{DT_RPATH, DT_RUNPATH};

    let mut bytes = fs::read(path).into_diagnostic()?;
    let patches = {
        let Ok(elf) = goblin::elf::Elf::parse(&bytes) else {
            return Ok(());
        };
        let Some(dynamic) = &elf.dynamic else {
            return Ok(());
        };
        // goblin maps DT_STRTAB to a file offset.
        let strtab = dynamic.info.strtab;
        let parent = path.parent().expect("file has a parent");
        let matches = |e: &str| patterns.iter().any(|p| e.contains(p.as_str()));

        let mut patches = Vec::new();
        for d in &dynamic.dyns {
            if d.d_tag != DT_RUNPATH && d.d_tag != DT_RPATH {
                continue;
            }
            let start = strtab + d.d_val as usize;
            let len = bytes[start..].iter().position(|&b| b == 0).unwrap_or(0);
            let old = String::from_utf8_lossy(&bytes[start..start + len]).to_string();
            if !old.split(':').any(matches) {
                continue;
            }
            let mut entries: Vec<String> = rpaths
                .iter()
                .map(|r| {
                    // Spelled exactly like the relinker does (`$ORIGIN/` for
                    // files in `lib/` itself), so it dedupes against it.
                    let rel = pathdiff::diff_paths(root.join(r), parent).unwrap_or_default();
                    format!("$ORIGIN/{}", rel.to_string_lossy())
                })
                .collect();
            for e in old.split(':') {
                if !e.is_empty() && !matches(e) && !entries.iter().any(|x| x == e) {
                    entries.push(e.to_string());
                }
            }
            let new = entries.join(":");
            if new.len() > len {
                miette::bail!("rewritten rpath {new:?} does not fit in place of {old:?}");
            }
            patches.push((start, len, new));
        }
        patches
    };
    if patches.is_empty() {
        return Ok(());
    }
    for (start, len, new) in patches {
        bytes[start..start + len].fill(0);
        bytes[start..start + new.len()].copy_from_slice(new.as_bytes());
    }
    let perms = fs::metadata(path).into_diagnostic()?.permissions();
    fs::write(path, bytes).into_diagnostic()?;
    fs::set_permissions(path, perms).into_diagnostic()?;
    Ok(())
}

/// Relink every binary among `files` so its rpaths point into the
/// environment (`$ORIGIN/../lib` & co.) instead of the build system's tree.
fn relink(manifest: &PackageManifest, root: &Path, files: &[PathBuf]) -> miette::Result<()> {
    if !manifest.relink || manifest.target_platform == Subdir::NoArch {
        return Ok(());
    }
    let system_tools = SystemTools::new("rattler-build", env!("CARGO_PKG_VERSION"));
    let allowlist = GlobVec::default();
    for file in files {
        if manifest.target_platform.is_linux() {
            if elf_without_rpath(file) {
                continue;
            }
            strip_build_rpaths(file, root, &manifest.strip_rpath_patterns, &manifest.rpaths)
                .with_context(|| format!("rewriting rpaths of {}", file.display()))?;
        }
        let relinker = match get_relinker(manifest.target_platform, file) {
            Ok(relinker) => relinker,
            Err(RelinkError::UnknownFileFormat) => continue,
            Err(err) => {
                return Err(err)
                    .into_diagnostic()
                    .with_context(|| format!("inspecting {}", file.display()));
            }
        };
        relinker
            .relink(root, root, &manifest.rpaths, &allowlist, &system_tools)
            .into_diagnostic()
            .with_context(|| format!("relinking {}", file.display()))?;
    }
    Ok(())
}

/// Build the package described by `manifest`; returns the package path.
pub fn create_from_manifest(
    manifest: &PackageManifest,
    output_dir: &Path,
) -> miette::Result<PathBuf> {
    let name = PackageName::try_from(manifest.name.as_str()).into_diagnostic()?;
    let version: VersionWithSource = manifest.version.parse().into_diagnostic()?;
    let noarch = manifest.noarch.unwrap_or(NoArchType::none());
    if noarch.is_none() == (manifest.target_platform == Subdir::NoArch) {
        miette::bail!(
            "`noarch` must be set exactly when `target_platform` is `noarch` (got target_platform={}, noarch={:?})",
            manifest.target_platform,
            manifest.noarch
        );
    }

    let staging = tempfile::tempdir().into_diagnostic()?;
    let staged = stage(manifest, staging.path())?;
    relink(manifest, staging.path(), &staged)?;

    let timestamp = manifest
        .timestamp
        .map(jiff::Timestamp::from_millisecond)
        .transpose()
        .into_diagnostic()?;

    let mut index =
        IndexJsonBuilder::new(name.clone(), version.clone(), manifest.build_string.clone())
            .with_build_number(manifest.build_number)
            .with_target_platform(&manifest.target_platform)
            .with_dependencies(manifest.depends.clone())
            .with_constraints(manifest.constrains.clone())
            .with_noarch(noarch);
    if let Some(license) = &manifest.license {
        index = index.with_license(license.clone());
    }
    if let Some(family) = &manifest.license_family {
        index = index.with_license_family(family.clone());
    }
    if let Some(ts) = timestamp {
        index = index.with_timestamp(ts);
    }
    for feature in &manifest.track_features {
        index = index.with_track_feature(feature.clone());
    }

    let mut about = AboutJsonBuilder::new();
    if let Some(v) = &manifest.summary {
        about = about.with_summary(v.clone());
    }
    if let Some(v) = &manifest.description {
        about = about.with_description(v.clone());
    }
    if let Some(v) = &manifest.license {
        about = about.with_license(v.clone());
    }
    if let Some(v) = &manifest.license_family {
        about = about.with_license_family(v.clone());
    }
    if let Some(v) = &manifest.homepage {
        about = about.with_homepage(v.clone());
    }
    if let Some(v) = &manifest.repository {
        about = about.with_dev_url(v.clone());
    }
    if let Some(v) = &manifest.documentation {
        about = about.with_doc_url(v.clone());
    }

    let config = PackageConfig {
        compression_level: manifest.compression_level,
        archive_type: ArchiveType::Conda,
        timestamp,
        compression_threads: manifest.compression_threads.max(1),
        detect_prefix: manifest.detect_prefix,
        store_recipe: false,
    };

    let mut builder = PackageBuilder::new(name, version, manifest.target_platform, config)
        .with_build_string(manifest.build_string.clone())
        .with_index(index.build().into_diagnostic()?)
        .with_about(about.build())
        .with_license_files(manifest.license_files.clone())
        .with_files_from_dir(staging.path())
        .into_diagnostic()?;
    if let Some(run_exports) = &manifest.run_exports {
        builder = builder.with_run_exports(run_exports.clone());
    }

    fs::create_dir_all(output_dir).into_diagnostic()?;
    let output = builder.build(output_dir).into_diagnostic()?;
    Ok(output.path)
}

/// Entry point of `rattler-build package create`.
pub fn create_package(opts: CreateOpts) -> miette::Result<()> {
    let content = fs::read_to_string(&opts.manifest).into_diagnostic()?;
    let manifest: PackageManifest = serde_json::from_str(&content)
        .into_diagnostic()
        .with_context(|| format!("parsing {}", opts.manifest.display()))?;
    let path = create_from_manifest(&manifest, &opts.output_dir)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rattler_conda_types::package::{IndexJson, PathType, PathsJson};
    use rattler_digest::{Sha256, compute_file_digest};

    use super::*;

    fn manifest(json: serde_json::Value) -> PackageManifest {
        serde_json::from_value(json).expect("valid manifest")
    }

    fn write(path: &Path, content: &str) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        path.to_path_buf()
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/binary_files")
            .join(name)
    }

    /// The RPATH / RUNPATH entries of an ELF file.
    fn rpaths(path: &Path) -> Vec<String> {
        let bytes = fs::read(path).unwrap();
        let elf = goblin::elf::Elf::parse(&bytes).unwrap();
        elf.rpaths
            .iter()
            .chain(elf.runpaths.iter())
            .flat_map(|r| r.split(':'))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn rejects_paths_outside_the_package() {
        assert!(check_dest("bin/tool").is_ok());
        assert!(check_dest("share/data/file.txt").is_ok());
        assert!(check_dest("/etc/passwd").is_err());
        assert!(check_dest("../outside").is_err());
        assert!(check_dest("bin/../../outside").is_err());
        assert!(check_dest("info/index.json").is_err());
    }

    #[test]
    fn creates_noarch_package_with_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let data = write(&tmp.path().join("src/greeting.txt"), "hello\n");
        let script = write(&tmp.path().join("src/greet.sh"), "#!/bin/sh\necho hi\n");
        let m = manifest(serde_json::json!({
            "name": "greet",
            "version": "1.2.3",
            "build_string": "h0123abcd_2",
            "build_number": 2,
            "target_platform": "noarch",
            "noarch": "generic",
            "depends": ["python >=3.10", "other-pkg"],
            "constrains": ["conflicting <1"],
            "license": "MIT",
            "summary": "A test package",
            "run_exports": {"weak": ["greet >=1.2.3"]},
            "timestamp": 1_672_531_200_000i64,
            "files": [
                {"src": data, "dest": "share/greet/greeting.txt"},
                {"src": script, "dest": "bin/greet", "executable": true}
            ],
            "symlinks": [{"dest": "bin/hi", "target": "greet"}]
        }));

        let out = create_from_manifest(&m, &tmp.path().join("out")).unwrap();
        assert_eq!(out.file_name().unwrap(), "greet-1.2.3-h0123abcd_2.conda");

        let index: IndexJson = rattler_package_streaming::seek::read_package_file(&out).unwrap();
        assert_eq!(index.name.as_normalized(), "greet");
        assert_eq!(index.build, "h0123abcd_2");
        assert_eq!(index.build_number, 2);
        assert_eq!(index.depends, vec!["python >=3.10", "other-pkg"]);
        assert_eq!(index.constrains, vec!["conflicting <1"]);
        assert_eq!(index.subdir.as_deref(), Some("noarch"));
        assert!(!index.noarch.is_none());

        let paths: PathsJson = rattler_package_streaming::seek::read_package_file(&out).unwrap();
        let mut entries: Vec<_> = paths
            .paths
            .iter()
            .map(|p| (p.relative_path.to_string_lossy().to_string(), p.path_type))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            entries,
            vec![
                ("bin/greet".to_string(), PathType::HardLink),
                ("bin/hi".to_string(), PathType::SoftLink),
                ("share/greet/greeting.txt".to_string(), PathType::HardLink),
            ]
        );
    }

    #[test]
    fn archives_are_reproducible() {
        let tmp = tempfile::tempdir().unwrap();
        let data = write(&tmp.path().join("data.txt"), "same content\n");
        let m = manifest(serde_json::json!({
            "name": "repro",
            "version": "0.1.0",
            "build_string": "0",
            "target_platform": "noarch",
            "noarch": "generic",
            "timestamp": 1_672_531_200_000i64,
            "files": [{"src": data, "dest": "share/repro/data.txt"}]
        }));
        let a = create_from_manifest(&m, &tmp.path().join("a")).unwrap();
        let b = create_from_manifest(&m, &tmp.path().join("b")).unwrap();
        assert_eq!(
            compute_file_digest::<Sha256>(&a).unwrap(),
            compute_file_digest::<Sha256>(&b).unwrap()
        );
    }

    #[test]
    fn noarch_must_match_target_platform() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest(serde_json::json!({
            "name": "mismatch",
            "version": "0.1.0",
            "build_string": "0",
            "target_platform": "linux-64",
            "noarch": "generic"
        }));
        assert!(create_from_manifest(&m, tmp.path()).is_err());
    }

    #[test]
    fn rejects_duplicate_destinations() {
        let tmp = tempfile::tempdir().unwrap();
        let a = write(&tmp.path().join("a.txt"), "a");
        let m = manifest(serde_json::json!({
            "name": "dup",
            "version": "0.1.0",
            "build_string": "0",
            "target_platform": "noarch",
            "noarch": "generic",
            "files": [{"src": a, "dest": "share/x"}],
            "symlinks": [{"dest": "share/x", "target": "y"}]
        }));
        assert!(create_from_manifest(&m, &tmp.path().join("out")).is_err());
    }

    /// Builds a linux-64 package with the `zlink-runpath` fixture as
    /// `bin/zlink` and returns its rpaths after installation.
    fn relinked_rpaths(extra: serde_json::Value) -> Vec<String> {
        let tmp = tempfile::tempdir().unwrap();
        let mut json = serde_json::json!({
            "name": "zlink",
            "version": "0.1.0",
            "build_string": "0",
            "target_platform": "linux-64",
            "files": [{"src": fixture("zlink-runpath"), "dest": "bin/zlink", "executable": true}]
        });
        json.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let out = create_from_manifest(&manifest(json), &tmp.path().join("out")).unwrap();
        let extracted = tmp.path().join("extracted");
        rattler_package_streaming::fs::extract(&out, &extracted).unwrap();
        rpaths(&extracted.join("bin/zlink"))
    }

    #[test]
    fn relinks_binaries_into_the_environment() {
        // The fixture's absolute build-prefix rpaths are outside the staged
        // package, so the relinker drops them and adds `$ORIGIN/../lib`.
        assert!(
            rpaths(&fixture("zlink-runpath"))
                .iter()
                .any(|r| r.contains("host_env_placehold"))
        );
        let rpaths = relinked_rpaths(serde_json::json!({}));
        assert!(rpaths.contains(&"$ORIGIN/../lib".to_string()), "{rpaths:?}");
        assert!(
            !rpaths.iter().any(|r| r.contains("host_env_placehold")),
            "{rpaths:?}"
        );
    }

    #[test]
    fn strips_build_system_rpaths() {
        // Entries matching `strip_rpath_patterns` are replaced in place.
        let rpaths = relinked_rpaths(serde_json::json!({
            "strip_rpath_patterns": ["host_env_placehold"]
        }));
        assert_eq!(rpaths, vec!["$ORIGIN/../lib".to_string()]);
    }

    #[test]
    fn relink_can_be_disabled() {
        let rpaths = relinked_rpaths(serde_json::json!({"relink": false}));
        assert!(
            rpaths.iter().any(|r| r.contains("host_env_placehold")),
            "{rpaths:?}"
        );
    }
}
