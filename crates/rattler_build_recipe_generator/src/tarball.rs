//! Helpers to look inside gzip-compressed source tarballs.

use std::io::Read as _;
use std::path::{Component, Path};

use miette::IntoDiagnostic;

/// Return the contents of the first file in the `.tar.gz` `tarball` whose
/// archive path satisfies `wanted`, or `None` when there is no such file.
pub fn find_file(
    tarball: &[u8],
    mut wanted: impl FnMut(&Path) -> bool,
) -> miette::Result<Option<String>> {
    let tar = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(tar);
    for entry in archive.entries().into_diagnostic()? {
        let mut entry = entry.into_diagnostic()?;
        if wanted(&entry.path().into_diagnostic()?) {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).into_diagnostic()?;
            return Ok(Some(contents));
        }
    }
    Ok(None)
}

/// Whether `path` is `<top-level directory>/<relative>`. Source tarballs
/// unpack into a single directory (named after the package), and a leading
/// `./` is ignored.
pub fn is_in_top_level_dir(path: &Path, relative: &str) -> bool {
    let mut components = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_)));
    components.next().is_some() && components.eq(Path::new(relative).components())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build an in-memory `.tar.gz` with the given `(path, contents)` entries.
    pub(crate) fn tarball_with(files: &[(&str, &str)]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn finds_the_first_matching_file() {
        let tarball = tarball_with(&[
            ("pkg/R/pkg.R", "NULL\n"),
            ("pkg/inst/extdata/DESCRIPTION", "Package: not-this-one\n"),
            ("pkg/DESCRIPTION", "Package: pkg\n"),
        ]);
        let description =
            find_file(&tarball, |path| is_in_top_level_dir(path, "DESCRIPTION")).unwrap();
        assert_eq!(description.as_deref(), Some("Package: pkg\n"));

        let missing = find_file(&tarball, |path| is_in_top_level_dir(path, "NEWS")).unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn top_level_dir_matching() {
        assert!(is_in_top_level_dir(
            Path::new("pkg/DESCRIPTION"),
            "DESCRIPTION"
        ));
        assert!(is_in_top_level_dir(
            Path::new("./pkg/tests/testthat.R"),
            "tests/testthat.R"
        ));
        assert!(!is_in_top_level_dir(
            Path::new("pkg/inst/DESCRIPTION"),
            "DESCRIPTION"
        ));
        assert!(!is_in_top_level_dir(
            Path::new("DESCRIPTION"),
            "DESCRIPTION"
        ));
    }
}
