//! Utility functions for working with paths.

use serde::{Deserialize, Serialize};
use serde_with::{formats::PreferOne, serde_as, OneOrMany};
use std::collections::btree_map::Entry;
use std::collections::btree_map::IntoIter;
use std::collections::BTreeMap;
use fs_err as fs;
use std::{
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

use miette::IntoDiagnostic;

/// Converts `p` to an absolute path, but doesn't resolve symlinks.
/// The function does normalize the path by resolving any `.` and `..` components which are present.
/// Usually, the `base_path` would be set to the current working directory.
pub fn to_lexical_absolute(p: &Path, base_path: &Path) -> PathBuf {
    let mut absolute = if p.is_absolute() {
        PathBuf::new()
    } else {
        base_path.to_path_buf()
    };
    for component in p.components() {
        match component {
            Component::CurDir => { /* do nothing for `.` components */ }
            Component::ParentDir => {
                // pop the last element that we added for `..` components
                absolute.pop();
            }
            // just push the component for any other component
            component => absolute.push(component.as_os_str()),
        }
    }
    absolute
}

/// Convert a path to a string with forward slashes (only on windows). Otherwise,
/// just return the path as a string.
pub fn to_forward_slash_lossy(path: &Path) -> std::borrow::Cow<'_, str> {
    #[cfg(target_os = "windows")]
    {
        let mut buf = String::new();
        for c in path.components() {
            match c {
                Component::RootDir => { /* root on windows can be skipped */ }
                Component::CurDir => buf.push('.'),
                Component::ParentDir => buf.push_str(".."),
                Component::Prefix(prefix) => {
                    buf.push_str(&prefix.as_os_str().to_string_lossy());
                    continue;
                }
                Component::Normal(s) => buf.push_str(&s.to_string_lossy()),
            }
            // use `/` instead of `\`
            buf.push('/');
        }

        fn ends_with_main_sep(p: &Path) -> bool {
            use std::os::windows::ffi::OsStrExt as _;
            p.as_os_str().encode_wide().last() == Some(std::path::MAIN_SEPARATOR as u16)
        }
        if buf != "/" && !ends_with_main_sep(path) && buf.ends_with('/') {
            buf.pop();
        }

        std::borrow::Cow::Owned(buf)
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string_lossy()
    }
}

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]

/// Struct where keys upon insertion and retrieval are normalized
pub struct NormalizedKeyBTreeMap {
    #[serde_as(deserialize_as = "BTreeMap<_, OneOrMany<_, PreferOne>>")]
    #[serde(flatten)]
    /// the inner map
    pub map: BTreeMap<String, Vec<String>>,
}

impl NormalizedKeyBTreeMap {
    /// Makes a new, empty `BTreeMap`
    pub fn new() -> Self {
        NormalizedKeyBTreeMap {
            map: BTreeMap::new(),
        }
    }

    /// Replaces all matches of a `-` with `_`.
    pub fn normalize_key(key: &str) -> String {
        key.replace('-', "_")
    }

    /// Inserts a key-value pair into the map, where key is normalized
    pub fn insert(&mut self, key: String, value: Vec<String>) {
        let normalized_key = Self::normalize_key(&key);
        self.map.insert(normalized_key, value);
    }

    /// Returns a reference to the value corresponding to the key.
    /// Key is normalized
    pub fn get(&self, key: &str) -> Option<&Vec<String>> {
        // Change value type as needed
        let normalized_key = Self::normalize_key(key);
        self.map.get(&normalized_key)
    }
}

impl Extend<(String, Vec<String>)> for NormalizedKeyBTreeMap {
    fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = (String, Vec<String>)>,
    {
        for (key, value) in iter {
            match self.map.entry(Self::normalize_key(&key)) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().extend(value);
                }
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
            }
        }
    }
}

impl NormalizedKeyBTreeMap {
    /// Gets an iterator over the entries of the map, sorted by key.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.map.iter()
    }
}

impl IntoIterator for NormalizedKeyBTreeMap {
    type Item = (String, Vec<String>);
    type IntoIter = IntoIter<String, Vec<String>>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a> IntoIterator for &'a NormalizedKeyBTreeMap {
    type Item = (&'a String, &'a Vec<String>);
    type IntoIter = std::collections::btree_map::Iter<'a, String, Vec<String>>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

/// Returns the UNIX epoch time in seconds.
pub fn get_current_timestamp() -> miette::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .into_diagnostic()?
        .as_secs())
}

/// Removes a directory and all its contents, including read-only files.
pub fn remove_dir_all_force(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // If the normal removal fails, try to forcefully remove it.
            tracing::debug!(
                "Adjusting permissions to remove read-only files in the build directory."
            );
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                let file_path = entry.path();
                let metadata = fs::metadata(file_path)?;
                let mut permissions = metadata.permissions();

                if permissions.readonly() {
                    permissions.set_readonly(false);
                    fs::set_permissions(file_path, permissions)?;
                }
            }
            fs::remove_dir_all(path)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_lexical_absolute() {
        let path = Path::new("/foo/bar/../baz");
        let absolute = to_lexical_absolute(path, &PathBuf::new());
        assert_eq!(absolute, Path::new("/foo/baz"));
    }

    #[test]
    fn test_to_forward_slash_lossy() {
        #[cfg(windows)]
        {
            let path = Path::new(r"C:\\foo\\bar\\baz");
            let forward_slash = to_forward_slash_lossy(path);
            assert_eq!(forward_slash, "C:/foo/bar/baz");
        }

        #[cfg(unix)]
        {
            let path = Path::new("/foo/bar/baz");
            let forward_slash = to_forward_slash_lossy(path);
            assert_eq!(forward_slash, "/foo/bar/baz");
        }
    }
}
