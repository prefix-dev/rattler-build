//! Persistent cache conditions for experimental named build steps.
//!
//! A step writes declarations to the file in `RATTLER_BUILD_STEP_CACHE`. After
//! a successful run we fingerprint the declared inputs and outputs. On the next
//! run the step can be skipped when every fingerprint is unchanged.

use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use globset::{GlobBuilder, GlobSetBuilder};
use rattler_digest::{HashingWriter, Sha256};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

const STATE_SUFFIX: &str = ".state.json";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Method {
    Hash,
    Mtime,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Side {
    Input,
    Output,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Condition {
    side: Side,
    method: Method,
    glob: String,
    fingerprint: String,
    matches: usize,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct CacheState {
    version: u8,
    step_identity: String,
    declarations: String,
    conditions: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq)]
struct Declaration {
    side: Side,
    method: Method,
    glob: String,
}

fn state_path(declaration_path: &Path) -> PathBuf {
    let mut name = declaration_path.as_os_str().to_os_string();
    name.push(STATE_SUFFIX);
    PathBuf::from(name)
}

fn parse_declarations(contents: &str) -> Result<Vec<Declaration>, std::io::Error> {
    let mut result = Vec::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, glob) = line.split_once(':').ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "invalid step cache declaration on line {}: expected `input-hash: GLOB`",
                    index + 1
                ),
            )
        })?;
        let (side, method) = match key.trim() {
            "input-hash" => (Side::Input, Method::Hash),
            "input-mtime" => (Side::Input, Method::Mtime),
            "output-hash" => (Side::Output, Method::Hash),
            "output-mtime" => (Side::Output, Method::Mtime),
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "unknown step cache condition `{other}` on line {}",
                        index + 1
                    ),
                ));
            }
        };
        let glob = glob.trim();
        if glob.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("empty step cache glob on line {}", index + 1),
            ));
        }
        let path = Path::new(glob);
        if path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "step cache glob must be relative and stay below the step working directory: `{glob}`"
                ),
            ));
        }
        result.push(Declaration {
            side,
            method,
            glob: glob.replace('\\', "/"),
        });
    }
    if result.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "step cache file contains no conditions",
        ));
    }
    Ok(result)
}

fn matching_paths(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, std::io::Error> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let matcher = builder
        .build()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut paths = Vec::new();
    if !root.is_dir() {
        return Ok(paths);
    }
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_type().is_dir() {
            continue;
        }
        let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
        if matcher.is_match(relative) {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn write_framed(writer: &mut impl std::io::Write, bytes: &[u8]) -> Result<(), std::io::Error> {
    writer.write_all(&(bytes.len() as u64).to_le_bytes())?;
    writer.write_all(bytes)
}

fn fingerprint(root: &Path, declaration: &Declaration) -> Result<(String, usize), std::io::Error> {
    let paths = matching_paths(root, &declaration.glob)?;
    let count = paths.len();
    let mut hasher = HashingWriter::<_, Sha256>::new(std::io::sink());
    for path in paths {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let relative = relative.to_string_lossy().replace('\\', "/");
        write_framed(&mut hasher, relative.as_bytes())?;
        match declaration.method {
            Method::Hash => {
                let metadata = fs_err::metadata(&path)?;
                std::io::Write::write_all(&mut hasher, &metadata.len().to_le_bytes())?;
                let mut file = File::open(&path)?;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    std::io::Write::write_all(&mut hasher, &buffer[..read])?;
                }
            }
            Method::Mtime => {
                let metadata = fs_err::symlink_metadata(&path)?;
                let modified = metadata
                    .modified()?
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default();
                std::io::Write::write_all(&mut hasher, &metadata.len().to_le_bytes())?;
                std::io::Write::write_all(&mut hasher, &modified.as_secs().to_le_bytes())?;
                std::io::Write::write_all(&mut hasher, &modified.subsec_nanos().to_le_bytes())?;
            }
        }
    }
    let (_, digest) = hasher.finalize();
    Ok((hex::encode(digest), count))
}

fn capture(root: &Path, contents: &str, step_identity: &str) -> Result<CacheState, std::io::Error> {
    let declarations = parse_declarations(contents)?;
    let mut conditions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let (fingerprint, matches) = fingerprint(root, &declaration)?;
        conditions.push(Condition {
            side: declaration.side,
            method: declaration.method,
            glob: declaration.glob,
            fingerprint,
            matches,
        });
    }
    Ok(CacheState {
        version: 1,
        step_identity: step_identity.to_string(),
        declarations: contents.to_string(),
        conditions,
    })
}

/// Return whether a step can be skipped using its declaration and saved state.
pub(crate) fn can_skip(
    declaration_path: &Path,
    root: &Path,
    step_identity: &str,
) -> Result<bool, std::io::Error> {
    let contents = match fs_err::read_to_string(declaration_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let saved: CacheState = match fs_err::read(state_path(declaration_path)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let current = capture(root, &contents, step_identity)?;
    // Missing matches are never a cache hit. This is especially important for
    // outputs: a deleted artifact must cause the producing step to rerun.
    Ok(saved == current && current.conditions.iter().all(|item| item.matches > 0))
}

/// Save fingerprints after a successful step. No declaration means no caching.
pub(crate) fn update(
    declaration_path: &Path,
    root: &Path,
    step_identity: &str,
) -> Result<(), std::io::Error> {
    let contents = match fs_err::read_to_string(declaration_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let _ = fs_err::remove_file(state_path(declaration_path));
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let state = capture(root, &contents, step_identity)?;
    let bytes = serde_json::to_vec_pretty(&state).map_err(std::io::Error::other)?;
    fs_err::write(state_path(declaration_path), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_mtime_conditions_invalidate_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs_err::create_dir_all(root.join("src")).unwrap();
        fs_err::create_dir_all(root.join("build")).unwrap();
        fs_err::write(root.join("src/main.c"), "one").unwrap();
        fs_err::write(root.join("build/app"), "artifact").unwrap();
        let declaration = root.join("step.cache");
        fs_err::write(
            &declaration,
            "# generated by the step\ninput-hash: src/**/*.c\noutput-mtime: build/**\n",
        )
        .unwrap();

        assert!(!can_skip(&declaration, root, "step-v1").unwrap());
        update(&declaration, root, "step-v1").unwrap();
        assert!(can_skip(&declaration, root, "step-v1").unwrap());
        assert!(!can_skip(&declaration, root, "step-v2").unwrap());

        fs_err::write(root.join("src/main.c"), "two").unwrap();
        assert!(!can_skip(&declaration, root, "step-v1").unwrap());
    }

    #[test]
    fn deleted_output_and_changed_declaration_invalidate_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs_err::write(root.join("input"), "input").unwrap();
        fs_err::write(root.join("output"), "output").unwrap();
        let declaration = root.join("step.cache");
        fs_err::write(&declaration, "input-hash: input\noutput-hash: output\n").unwrap();
        update(&declaration, root, "step").unwrap();
        assert!(can_skip(&declaration, root, "step").unwrap());

        fs_err::remove_file(root.join("output")).unwrap();
        assert!(!can_skip(&declaration, root, "step").unwrap());
        fs_err::write(root.join("output"), "output").unwrap();
        fs_err::write(&declaration, "input-mtime: input\noutput-hash: output\n").unwrap();
        assert!(!can_skip(&declaration, root, "step").unwrap());
    }

    #[test]
    fn rejects_unsafe_and_unknown_declarations() {
        assert!(parse_declarations("input-hash: ../secret\n").is_err());
        assert!(parse_declarations("wat: src/**\n").is_err());
        assert!(parse_declarations("# only a comment\n").is_err());
    }

    #[test]
    fn hash_fingerprint_frames_paths_and_contents() {
        let first = tempfile::tempdir().unwrap();
        fs_err::write(first.path().join("a"), "bc").unwrap();
        let second = tempfile::tempdir().unwrap();
        fs_err::write(second.path().join("ab"), "c").unwrap();
        let declaration = Declaration {
            side: Side::Input,
            method: Method::Hash,
            glob: "*".to_string(),
        };

        assert_ne!(
            fingerprint(first.path(), &declaration).unwrap(),
            fingerprint(second.path(), &declaration).unwrap()
        );
    }
}
