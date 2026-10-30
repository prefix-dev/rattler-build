//! Helpers to look inside gzip-compressed source tarballs.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use miette::IntoDiagnostic;

/// Download the archive at `url`, failing on HTTP error statuses so that an
/// error page is never mistaken for the archive itself.
pub async fn download(
    client: &reqwest::Client,
    url: impl reqwest::IntoUrl,
) -> miette::Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .into_diagnostic()?
        .error_for_status()
        .into_diagnostic()?;
    Ok(response.bytes().await.into_diagnostic()?.into())
}

/// The contents of the archive files named by `relatives`, keyed by the entry
/// of `relatives` that matched (see [`is_archive_file`]). Missing files are
/// simply absent from the result.
///
/// Contents are read lossily: R for example allows latin1-encoded files, and a
/// stray byte must not make the whole file unavailable.
pub fn find_archive_files<'a>(
    tarball: &[u8],
    relatives: &[&'a str],
) -> miette::Result<HashMap<&'a str, String>> {
    Ok(scan_archive(tarball, relatives)?.files)
}

/// What a single decompression pass found in a source archive.
#[derive(Debug, Default)]
pub struct ArchiveScan<'a> {
    /// The requested files, as returned by [`find_archive_files`].
    pub files: HashMap<&'a str, String>,
    /// The paths of all regular files in the archive, as stored in it.
    pub paths: Vec<PathBuf>,
}

/// [`find_archive_files`] that also lists every regular file of the archive,
/// so that a caller can look at its layout (see [`strip_archive_prefix`])
/// without a second decompression pass.
pub fn scan_archive<'a>(tarball: &[u8], relatives: &[&'a str]) -> miette::Result<ArchiveScan<'a>> {
    let tar = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(tar);
    let mut scan = ArchiveScan::default();
    for entry in archive.entries().into_diagnostic()? {
        let mut entry = entry.into_diagnostic()?;
        let path = entry.path().into_diagnostic()?.into_owned();
        let Some(relative) = relatives
            .iter()
            .copied()
            .find(|relative| is_archive_file(&path, relative))
        else {
            if entry.header().entry_type().is_file() {
                scan.paths.push(path);
            }
            continue;
        };
        if !scan.files.contains_key(relative) {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents).into_diagnostic()?;
            scan.files
                .insert(relative, String::from_utf8_lossy(&contents).into_owned());
        }
        scan.paths.push(path);
    }
    Ok(scan)
}

/// What follows `<relative>` in `path`, when `path` starts with it either at
/// the archive root or directly inside the single top-level directory that
/// source archives usually unpack into. Anything nested deeper (a vendored or
/// test copy) does not match, and a leading `./` is ignored.
pub fn strip_archive_prefix(path: &Path, relative: &str) -> Option<PathBuf> {
    let components: Vec<_> = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .collect();
    let wanted: Vec<_> = Path::new(relative).components().collect();
    (0..=1).find_map(|leading| {
        let rest = components.get(leading..)?.strip_prefix(&wanted[..])?;
        Some(rest.iter().collect())
    })
}

/// Whether `path` is `<relative>` (see [`strip_archive_prefix`]).
fn is_archive_file(path: &Path, relative: &str) -> bool {
    strip_archive_prefix(path, relative).is_some_and(|rest| rest.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory `.tar.gz` with the given `(path, contents)` entries.
    fn tarball_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn finds_the_requested_files() {
        let tarball = tarball_with(&[
            ("pkg/R/pkg.R", b"NULL\n"),
            ("pkg/inst/extdata/DESCRIPTION", b"Package: not-this-one\n"),
            ("pkg/DESCRIPTION", b"Package: pkg\n"),
        ]);
        let found = find_archive_files(&tarball, &["DESCRIPTION", "NEWS"]).unwrap();
        assert_eq!(
            found.get("DESCRIPTION").map(String::as_str),
            Some("Package: pkg\n")
        );
        assert_eq!(found.get("NEWS"), None);
    }

    /// R permits e.g. `Encoding: latin1` DESCRIPTION files.
    #[test]
    fn non_utf8_contents_are_read_lossily() {
        // latin1 e-diaeresis
        let tarball = tarball_with(&[("pkg/DESCRIPTION", b"Author: Ga\xEBl\n")]);

        let found = find_archive_files(&tarball, &["DESCRIPTION"]).unwrap();
        assert_eq!(
            found.get("DESCRIPTION").map(String::as_str),
            Some("Author: Ga\u{FFFD}l\n")
        );
    }

    #[test]
    fn lists_the_files_in_the_same_pass() {
        let tarball = tarball_with(&[
            ("pkg/DESCRIPTION", b"Package: pkg\n"),
            ("pkg/src/init.c", b""),
            ("pkg/src/lapack/dgesv.f", b""),
        ]);
        let scan = scan_archive(&tarball, &["DESCRIPTION"]).unwrap();
        assert!(scan.files.contains_key("DESCRIPTION"));
        assert_eq!(
            scan.paths,
            [
                PathBuf::from("pkg/DESCRIPTION"),
                PathBuf::from("pkg/src/init.c"),
                PathBuf::from("pkg/src/lapack/dgesv.f"),
            ]
        );
    }

    #[test]
    fn archive_prefix_stripping() {
        let inside = |path: &str| strip_archive_prefix(Path::new(path), "src");
        // Inside the usual top-level directory, and at the archive root.
        assert_eq!(inside("pkg/src/init.c"), Some(PathBuf::from("init.c")));
        assert_eq!(inside("./src/a/b.f"), Some(PathBuf::from("a/b.f")));
        // The directory itself, and one nested deeper.
        assert_eq!(inside("pkg/src"), Some(PathBuf::new()));
        assert_eq!(inside("pkg/inst/src/unused.f"), None);
        // A prefix of several components.
        assert_eq!(
            strip_archive_prefix(Path::new("pkg/src/lapack/dgesv.f"), "src/lapack"),
            Some(PathBuf::from("dgesv.f"))
        );
    }

    #[test]
    fn archive_file_matching() {
        // Inside the usual top-level directory, and at the archive root.
        assert!(is_archive_file(Path::new("pkg/DESCRIPTION"), "DESCRIPTION"));
        assert!(is_archive_file(Path::new("DESCRIPTION"), "DESCRIPTION"));
        assert!(is_archive_file(
            Path::new("./pkg/tests/testthat.R"),
            "tests/testthat.R"
        ));
        assert!(is_archive_file(
            Path::new("tests/testthat.R"),
            "tests/testthat.R"
        ));
        // Nested deeper: a vendored or test copy.
        assert!(!is_archive_file(
            Path::new("pkg/inst/DESCRIPTION"),
            "DESCRIPTION"
        ));
        assert!(!is_archive_file(
            Path::new("pkg/docs/tests/testthat.R"),
            "tests/testthat.R"
        ));
        assert!(!is_archive_file(Path::new("pkg/NEWS"), "DESCRIPTION"));
    }
}
