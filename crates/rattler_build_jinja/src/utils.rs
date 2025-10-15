//! Utility functions for jinja operations
use std::path::Path;

#[cfg(target_os = "windows")]
use std::path::Component;

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
