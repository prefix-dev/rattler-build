//! File transformation utilities for package creation

use std::path::{Component, Path, PathBuf};

/// Transforms file paths for packaging
///
/// This handles transformations like:
/// - Stripping site-packages prefix for noarch python
/// - Making symlinks relative
/// - Replacing bin/ with python-scripts/ for noarch python
pub struct FileTransformer {
    /// Whether to strip site-packages prefix
    strip_site_packages: bool,

    /// Whether to make absolute symlinks relative
    relativize_symlinks: bool,

    /// Whether to transform bin/ to python-scripts/
    transform_bin_to_python_scripts: bool,
}

impl FileTransformer {
    /// Create a new FileTransformer with default settings
    pub fn new() -> Self {
        Self {
            strip_site_packages: false,
            relativize_symlinks: true,
            transform_bin_to_python_scripts: false,
        }
    }

    /// Enable site-packages stripping (for noarch: python)
    pub fn with_strip_site_packages(mut self, strip: bool) -> Self {
        self.strip_site_packages = strip;
        self
    }

    /// Enable symlink relativization
    pub fn with_relativize_symlinks(mut self, relativize: bool) -> Self {
        self.relativize_symlinks = relativize;
        self
    }

    /// Enable bin/ to python-scripts/ transformation (for noarch: python)
    pub fn with_transform_bin(mut self, transform: bool) -> Self {
        self.transform_bin_to_python_scripts = transform;
        self
    }

    /// Transform a path according to the configured rules
    ///
    /// Returns None if the file should be skipped
    pub fn transform_path(&self, path: &Path) -> Option<PathBuf> {
        let mut result = path.to_path_buf();

        // Strip site-packages prefix if configured
        if self.strip_site_packages {
            result = self.strip_to_site_packages(&result)?;
        }

        // Transform bin/ to python-scripts/ if configured
        if self.transform_bin_to_python_scripts {
            result = self.transform_bin_dir(&result);
        }

        Some(result)
    }

    /// Strip everything before site-packages in the path
    fn strip_to_site_packages(&self, path: &Path) -> Option<PathBuf> {
        let mut components = Vec::new();
        let mut found_site_packages = false;

        for component in path.components() {
            if component == Component::Normal("site-packages".as_ref()) {
                found_site_packages = true;
            }

            if found_site_packages {
                components.push(component);
            }
        }

        if found_site_packages {
            Some(components.iter().collect())
        } else {
            Some(path.to_path_buf())
        }
    }

    /// Transform bin/ or Scripts/ to python-scripts/
    fn transform_bin_dir(&self, path: &Path) -> PathBuf {
        let mut components: Vec<Component> = path.components().collect();

        // TODO(refactor): Do we need to handle `-script` suffixes?
        if let Some(first) = components.first()
            && (first == &Component::Normal("bin".as_ref())
                || first == &Component::Normal("Scripts".as_ref()))
        {
            components[0] = Component::Normal("python-scripts".as_ref());
        }

        components.iter().collect()
    }

    /// Make a symlink target relative to its location
    ///
    /// If the target is absolute and within the prefix, convert it to relative.
    pub fn relativize_symlink(&self, symlink_path: &Path, target: &Path, prefix: &Path) -> PathBuf {
        if !self.relativize_symlinks {
            return target.to_path_buf();
        }

        // If target is not absolute, keep as-is
        if !target.is_absolute() {
            return target.to_path_buf();
        }

        // If target is not within prefix, keep as absolute (with warning)
        if !target.starts_with(prefix) {
            tracing::warn!(
                "Symlink {:?} points to absolute path {:?} outside of prefix",
                symlink_path,
                target
            );
            return target.to_path_buf();
        }

        // Make relative
        if let Some(parent) = symlink_path.parent()
            && let Some(rel) = pathdiff::diff_paths(target, parent)
        {
            return rel;
        }

        target.to_path_buf()
    }
}

impl Default for FileTransformer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_site_packages() {
        let transformer = FileTransformer::new().with_strip_site_packages(true);

        let path = Path::new("lib/python3.11/site-packages/mypackage/__init__.py");
        let result = transformer.transform_path(path).unwrap();

        assert_eq!(result, PathBuf::from("site-packages/mypackage/__init__.py"));
    }

    #[test]
    fn test_transform_bin() {
        let transformer = FileTransformer::new().with_transform_bin(true);

        let path = Path::new("bin/my-script");
        let result = transformer.transform_path(path).unwrap();

        assert_eq!(result, PathBuf::from("python-scripts/my-script"));
    }

    #[test]
    #[cfg(unix)]
    fn test_relativize_symlink() {
        let transformer = FileTransformer::new();

        let prefix = Path::new("/prefix");
        let symlink = Path::new("/prefix/bin/tool");
        let target = Path::new("/prefix/lib/libtool.so");

        let result = transformer.relativize_symlink(symlink, target, prefix);

        assert_eq!(result, PathBuf::from("../lib/libtool.so"));
    }
}
