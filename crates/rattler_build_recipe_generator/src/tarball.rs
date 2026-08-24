//! Helpers to look inside gzip-compressed source tarballs.

use std::io::Read as _;
use std::path::{Component, Path};

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

/// Return the contents of the first file in the `.tar.gz` `tarball` whose
/// archive path satisfies `wanted`, or `None` when there is no such file.
/// Contents are read lossily: R for example allows latin1-encoded files, and
/// a stray byte must not make the whole file unavailable.
pub fn find_file(
    tarball: &[u8],
    wanted: impl FnMut(&Path) -> bool,
) -> miette::Result<Option<String>> {
    Ok(find_files(tarball, wanted, 1)?
        .pop()
        .map(|(_, contents)| contents))
}

/// The paths and contents of the files matching `wanted`, collected in a
/// single decompression pass that stops once `limit` files have been found.
/// See [`find_file`] for the lossy read semantics.
pub fn find_files(
    tarball: &[u8],
    mut wanted: impl FnMut(&Path) -> bool,
    limit: usize,
) -> miette::Result<Vec<(std::path::PathBuf, String)>> {
    let tar = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(tar);
    let mut found = Vec::new();
    for entry in archive.entries().into_diagnostic()? {
        if found.len() >= limit {
            break;
        }
        let mut entry = entry.into_diagnostic()?;
        let path = entry.path().into_diagnostic()?.into_owned();
        if wanted(&path) {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents).into_diagnostic()?;
            found.push((path, String::from_utf8_lossy(&contents).into_owned()));
        }
    }
    Ok(found)
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

    /// R permits e.g. `Encoding: latin1` DESCRIPTION files.
    #[test]
    fn non_utf8_contents_are_read_lossily() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let contents = b"Author: Ga\xEBl\n"; // latin1 e-diaeresis
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "pkg/DESCRIPTION", &contents[..])
            .unwrap();
        let tarball = builder.into_inner().unwrap().finish().unwrap();

        let description =
            find_file(&tarball, |path| is_in_top_level_dir(path, "DESCRIPTION")).unwrap();
        assert_eq!(description.as_deref(), Some("Author: Ga\u{FFFD}l\n"));
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
