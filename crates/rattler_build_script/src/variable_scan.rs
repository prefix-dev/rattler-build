//! Best-effort detection of variant variables referenced by build scripts.
//!
//! Recipes frequently consume variant variables directly from the environment
//! of their build script (e.g. `${TARGET}` in `build.sh`) without ever
//! referencing them in the recipe itself. Since variant values are exported to
//! the build script environment under their (normalized) key names, a script
//! that mentions a variant key is very likely using it.
//!
//! The detection is deliberately simple: given the list of variant
//! configuration keys, we search the script text for **literal occurrences**
//! of each key name, bounded by non-identifier characters. There is no
//! per-interpreter syntax parsing — `$TARGET`, `%TARGET%`,
//! `os.environ["TARGET"]`, and even a plain mention of `TARGET` all count.
//! This over-approximates (a key mentioned in a comment matches too), which is
//! fine: erring towards "the variable is defined and exported" is safer than
//! silently expanding to an empty string, and only names that the user
//! explicitly declared in the variant configuration can ever match.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{Script, ScriptContent};

/// Returns true for characters that can be part of an environment variable
/// identifier. A key match must not be surrounded by these characters, so
/// that the key `TARGET` does not match inside `TARGET_ARCH` or `MYTARGET`.
fn is_identifier_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Check whether `key` occurs in `content` as a standalone word
/// (i.e. not embedded in a longer identifier). Matching is case-sensitive.
fn contains_key(content: &str, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let bytes = content.as_bytes();
    for (idx, _) in content.match_indices(key) {
        let before_ok = idx == 0 || !is_identifier_char(bytes[idx - 1]);
        let end = idx + key.len();
        let after_ok = end >= bytes.len() || !is_identifier_char(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Return the subset of `candidates` that occur literally (word-bounded,
/// case-sensitive) in `content`.
pub fn find_referenced_variables<'a>(
    content: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<String> {
    candidates
        .into_iter()
        .filter(|key| contains_key(content, key))
        .map(str::to_string)
        .collect()
}

/// Returns the script file extensions to try for a target platform, in
/// priority order (mirrors [`crate::platform_script_extensions`], but for an
/// explicit platform instead of the compile-time host).
pub fn script_extensions_for_platform(is_windows: bool) -> &'static [&'static str] {
    if is_windows { &["bat", "ps1"] } else { &["sh"] }
}

/// Resolve a script path the same way script execution does: relative paths
/// are anchored at the recipe directory and extension-less paths try the
/// platform extensions in order.
fn find_script_file(recipe_dir: &Path, extensions: &[&str], path: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        recipe_dir.join(path)
    };

    if path.extension().is_none() {
        extensions
            .iter()
            .map(|ext| path.with_extension(ext))
            .find(|p| p.is_file())
    } else if path.is_file() {
        Some(path)
    } else {
        None
    }
}

impl Script {
    /// Detect which of the `candidates` (variant configuration key names, in
    /// their normalized spelling) are referenced by this script.
    ///
    /// This is a best-effort, read-only literal search (see the module docs):
    /// - inline content is searched directly,
    /// - file-backed scripts (including the default `build.sh` / `build.bat`
    ///   discovery) are read from `recipe_dir` when it is provided,
    /// - missing or unreadable files simply contribute no variables.
    pub fn detect_used_variables(
        &self,
        candidates: &[String],
        recipe_dir: Option<&Path>,
        is_windows: bool,
    ) -> BTreeSet<String> {
        if candidates.is_empty() {
            return BTreeSet::new();
        }
        let extensions = script_extensions_for_platform(is_windows);
        let candidates = candidates.iter().map(String::as_str);

        let scan_file = |path: &Path| -> Option<BTreeSet<String>> {
            let recipe_dir = recipe_dir?;
            let resolved = find_script_file(recipe_dir, extensions, path)?;
            let content = fs_err::read_to_string(&resolved).ok()?;
            Some(find_referenced_variables(&content, candidates.clone()))
        };

        match &self.content {
            ScriptContent::Default => scan_file(Path::new("build")).unwrap_or_default(),
            ScriptContent::Path(path) => scan_file(path).unwrap_or_default(),
            ScriptContent::CommandOrPath(command_or_path) => {
                // Single-line strings may reference a script file; multi-line
                // strings are always inline content.
                if !command_or_path.contains('\n')
                    && let Some(from_file) = scan_file(Path::new(command_or_path))
                {
                    return from_file;
                }
                find_referenced_variables(command_or_path, candidates)
            }
            ScriptContent::Commands(commands) => {
                let mut variables = BTreeSet::new();
                for command in commands {
                    variables.extend(find_referenced_variables(command, candidates.clone()));
                }
                variables
            }
            ScriptContent::Command(command) => find_referenced_variables(command, candidates),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(content: &str, candidates: &[&str]) -> Vec<String> {
        find_referenced_variables(content, candidates.iter().copied())
            .into_iter()
            .collect()
    }

    #[test]
    fn matches_common_spellings_across_interpreters() {
        let candidates = &["TARGET", "PREFIX", "MY_VAR"];
        // Any interpreter syntax matches, because the search is literal.
        assert_eq!(find("cmake -DT=${TARGET} ..", candidates), ["TARGET"]);
        assert_eq!(find("echo $TARGET", candidates), ["TARGET"]);
        assert_eq!(find("echo %TARGET%", candidates), ["TARGET"]);
        assert_eq!(find("echo !TARGET!", candidates), ["TARGET"]);
        assert_eq!(find("$env:TARGET", candidates), ["TARGET"]);
        assert_eq!(find("os.environ['TARGET']", candidates), ["TARGET"]);
        assert_eq!(find("ENV.fetch('TARGET')", candidates), ["TARGET"]);
        assert_eq!(find("$env.TARGET", candidates), ["TARGET"]);
        assert_eq!(find("process.env.TARGET", candidates), ["TARGET"]);
        assert_eq!(
            find("install --prefix=$PREFIX --target=${TARGET}", candidates),
            ["PREFIX", "TARGET"]
        );
    }

    #[test]
    fn plain_mentions_match_too() {
        // Deliberately "not smart": a bare mention (even in a comment) counts.
        assert_eq!(
            find("# adjust TARGET before building", &["TARGET"]),
            ["TARGET"]
        );
    }

    #[test]
    fn respects_identifier_boundaries_and_case() {
        let candidates = &["TARGET"];
        // Not standalone: embedded in longer identifiers.
        assert!(find("echo $TARGET_ARCH", candidates).is_empty());
        assert!(find("echo $MYTARGET", candidates).is_empty());
        assert!(find("echo TARGET2", candidates).is_empty());
        // Case-sensitive: variant values are exported under the exact key name.
        assert!(find("echo $target", candidates).is_empty());
        // Standalone at string edges.
        assert_eq!(find("TARGET", candidates), ["TARGET"]);
        assert_eq!(find("x=TARGET", candidates), ["TARGET"]);
    }

    #[test]
    fn normalized_lowercase_keys_match() {
        // Free-spec style keys (e.g. package names) use their normalized form.
        assert_eq!(
            find("echo $cuda_compiler_version", &["cuda_compiler_version"]),
            ["cuda_compiler_version"]
        );
    }

    #[test]
    fn empty_candidates_and_empty_keys_are_safe() {
        assert!(find("echo $TARGET", &[]).is_empty());
        assert!(find("echo $TARGET", &[""]).is_empty());
    }

    #[test]
    fn default_script_discovery_scans_build_sh() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(
            dir.path().join("build.sh"),
            "cmake -DCMAKE_C_COMPILER_TARGET=${TARGET} ..\n",
        )
        .unwrap();

        let candidates = vec!["TARGET".to_string(), "UNUSED".to_string()];
        let script = Script::default();
        let vars = script.detect_used_variables(&candidates, Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["TARGET"]);

        // Without a recipe dir nothing is scanned.
        let vars = script.detect_used_variables(&candidates, None, false);
        assert!(vars.is_empty());
    }

    #[test]
    fn default_script_discovery_scans_build_bat_on_windows_target() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(
            dir.path().join("build.bat"),
            "cmake -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% -DTARGET=%TARGET% ..\r\n",
        )
        .unwrap();

        let candidates = vec!["TARGET".to_string(), "LIBRARY_PREFIX".to_string()];
        let script = Script::default();
        let vars = script.detect_used_variables(&candidates, Some(dir.path()), true);
        assert_eq!(
            vars.into_iter().collect::<Vec<_>>(),
            ["LIBRARY_PREFIX", "TARGET"]
        );
    }

    #[test]
    fn explicit_file_is_scanned() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(
            dir.path().join("install.py"),
            "import os\nprint(os.environ['TARGET'])\n",
        )
        .unwrap();

        let candidates = vec!["TARGET".to_string()];
        let script = Script {
            content: ScriptContent::Path(PathBuf::from("install.py")),
            ..Default::default()
        };
        let vars = script.detect_used_variables(&candidates, Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["TARGET"]);
    }

    #[test]
    fn command_or_path_resolves_files_and_falls_back_to_inline() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(dir.path().join("build.sh"), "echo ${FROM_FILE}\n").unwrap();

        let candidates = vec!["FROM_FILE".to_string(), "INLINE_VAR".to_string()];

        // Resolves to the file next to the recipe.
        let script = Script {
            content: ScriptContent::CommandOrPath("build.sh".to_string()),
            ..Default::default()
        };
        let vars = script.detect_used_variables(&candidates, Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["FROM_FILE"]);

        // Not a file: treated as an inline command.
        let script = Script {
            content: ScriptContent::CommandOrPath("echo $INLINE_VAR".to_string()),
            ..Default::default()
        };
        let vars = script.detect_used_variables(&candidates, Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["INLINE_VAR"]);
    }

    #[test]
    fn inline_commands_are_scanned() {
        let script = Script {
            content: ScriptContent::Commands(vec![
                "import os".to_string(),
                "print(os.environ['TARGET'])".to_string(),
            ]),
            ..Default::default()
        };
        let vars = script.detect_used_variables(&["TARGET".to_string()], None, false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["TARGET"]);
    }
}
