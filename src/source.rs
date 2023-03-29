use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

use rattler_digest::compute_file_digest;
use url::Url;

use super::metadata::{Checksum, GitSrc, Source, UrlSrc};

fn validate_checksum(path: &Path, checksum: &Checksum) -> bool {
    match checksum {
        Checksum::Sha256(value) => {
            let digest =
                compute_file_digest::<sha2::Sha256>(path).expect("Could not compute SHA256");
            let computed_sha = hex::encode(digest);
            if !computed_sha.eq(value) {
                tracing::error!(
                    "SHA256 values of downloaded file not matching!\nDownloaded = {}, should be {}",
                    computed_sha,
                    value
                );
                false
            } else {
                tracing::info!("Validated SHA256 values of the downloaded file!");
                true
            }
        }
        Checksum::Md5(_value) => {
            todo!("MD5 not implemented yet!");
        }
    }
}

fn split_filename(filename: &str) -> (String, String) {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    let stem_without_tar = stem.trim_end_matches(".tar");

    let extension = Path::new(filename)
        .extension()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    let full_extension = if stem != stem_without_tar {
        format!(".tar.{}", extension)
    } else if !extension.is_empty() {
        format!(".{}", extension)
    } else {
        "".to_string()
    };

    (stem_without_tar.to_string(), full_extension)
}

fn cache_name_from_url(url: &Url, checksum: &Checksum) -> String {
    let filename = url.path_segments().unwrap().last().unwrap();
    let (stem, extension) = split_filename(filename);
    let checksum = match checksum {
        Checksum::Sha256(value) => value,
        Checksum::Md5(value) => value,
    };
    format!("{}_{}{}", stem, &checksum[0..8], extension)
}

async fn url_src(
    source: &UrlSrc,
    cache_dir: &Path,
    checksum: &Checksum,
) -> anyhow::Result<PathBuf> {
    let cache_src = cache_dir.join("src_cache");
    fs::create_dir_all(&cache_src)?;

    let cache_name = PathBuf::from(cache_name_from_url(&source.url, checksum));
    let cache_name = cache_src.join(cache_name);

    let metadata = fs::metadata(&cache_name);
    if metadata.is_ok() && metadata?.is_file() && validate_checksum(&cache_name, checksum) {
        tracing::info!("Found valid source cache file.");
        return Ok(cache_name.clone());
    }

    let response = reqwest::get(source.url.clone()).await?;

    let mut file = std::fs::File::create(&cache_name)?;
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut file)?;
    Ok(cache_name)
}

fn git_src(_source: &GitSrc) {}

fn extract(
    archive: &Path,
    target_directory: &Path,
) -> Result<std::process::Output, std::io::Error> {
    let output = Command::new("tar")
        .arg("-xf")
        .arg(String::from(archive.to_string_lossy()))
        .arg("--preserve-permissions")
        .arg("--strip-components=1")
        .arg("-C")
        .arg(String::from(target_directory.to_string_lossy()))
        .output();

    output
}

pub fn apply_patches(
    patches: &[PathBuf],
    work_dir: &Path,
    recipe_dir: &Path,
) -> anyhow::Result<()> {
    for patch in patches {
        let patch = recipe_dir.join(patch);
        let output = Command::new("patch")
            .arg("-p1")
            .arg("-i")
            .arg(String::from(patch.to_string_lossy()))
            .arg("-d")
            .arg(String::from(work_dir.to_string_lossy()))
            .output()?;

        if !output.status.success() {
            tracing::error!("Failed to apply patch: {}", patch.to_string_lossy());
            tracing::error!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
            tracing::error!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
            anyhow::bail!("Failed to apply patch: {}", patch.to_string_lossy());
        }
    }
    Ok(())
}

pub async fn fetch_sources(
    sources: &[Source],
    work_dir: &Path,
    recipe_dir: &Path,
) -> anyhow::Result<()> {
    let cache_dir = std::env::current_dir()?.join("ROAR_CACHE");
    fs::create_dir_all(&cache_dir)?;
    for src in sources {
        match &src {
            Source::Git(src) => {
                tracing::info!("Fetching source from GIT: {}", src.git_src);
                git_src(src);
                if let Some(patches) = &src.patches {
                    apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url);
                let res = url_src(src, &cache_dir, &src.checksum).await?;
                extract(&res, work_dir).expect("Could not extract the file!");
                tracing::info!("Extracted to {:?}", work_dir);
                if let Some(patches) = &src.patches {
                    apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_filename() {
        let test_cases = vec![
            ("example.tar.gz", ("example", ".tar.gz")),
            ("example.tar.bz2", ("example", ".tar.bz2")),
            ("example.zip", ("example", ".zip")),
            ("example.tar", ("example", ".tar")),
            ("example", ("example", "")),
            (".hidden.tar.gz", (".hidden", ".tar.gz")),
        ];

        for (filename, expected) in test_cases {
            let (name, ending) = split_filename(filename);
            assert_eq!(
                (name.as_str(), ending.as_str()),
                expected,
                "Failed for filename: {}",
                filename
            );
        }
    }

    #[test]
    fn test_cache_name() {
        let cases = vec![
            (
                "https://example.com/example.tar.gz",
                Checksum::Sha256(
                    "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
                ),
                "example_12345678.tar.gz",
            ),
            (
                "https://github.com/mamba-org/mamba/archive/refs/tags/micromamba-12.23.12.tar.gz",
                Checksum::Sha256(
                    "63fd8a1dbec811e63d4f9b5e27757af45d08a219d0900c7c7a19e0b177a576b8".to_string(),
                ),
                "micromamba-12.23.12_63fd8a1d.tar.gz",
            ),
        ];

        for (url, checksum, expected) in cases {
            let url = Url::parse(url).unwrap();
            let name = cache_name_from_url(&url, &checksum);
            assert_eq!(name, expected);
        }
    }
}
