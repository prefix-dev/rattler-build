use std::{fs, io::Cursor, path::PathBuf, process::Command};

use reqwest;

use crate::hash::sha256_digest;

use super::metadata::{Checksum, GitSrc, Source, UrlSrc};

fn validate_checksum(path: &PathBuf, checksum: &Checksum) {
    match checksum {
        Checksum::sha256(value) => {
            let computed_sha = sha256_digest(&path);
            if (!computed_sha.eq(value)) {
                eprintln!(
                    "SHA256 values of downloaded file not matching!\nDownloaded = {}, should be {}",
                    computed_sha, value
                );
            } else {
                println!("Validated SHA256 values of the downloaded file!");
            }
        }
        Checksum::md5(value) => {
            eprintln!("MD5 not implemented yet!");
        }
    }
}

async fn url_src(source: &UrlSrc, cache_dir: &PathBuf) -> anyhow::Result<PathBuf> {
    let response = reqwest::get(&source.url).await?;

    let cache_src = cache_dir.join("src_cache");
    fs::create_dir_all(&cache_src);

    let cache_name = cache_src.join("file.tar.gz");
    println!("Cache file is: {:?}", cache_name);

    let mut file = std::fs::File::create(&cache_name)?;
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut file)?;
    Ok(cache_name)
}

fn git_src(source: &GitSrc) {}

fn extract(
    archive: &PathBuf,
    target_directory: &PathBuf,
) -> Result<std::process::Output, std::io::Error> {
    // tar -xf file.name.tar -C /path/to/directory
    println!(
        "tar -xf {} -C {}",
        archive.to_string_lossy(),
        target_directory.to_string_lossy()
    );
    let output = Command::new("tar")
        .arg("-xf")
        .arg(String::from(archive.to_string_lossy()))
        .arg("--preserve")
        .arg("--strip-components=1")
        .arg("-C")
        .arg(String::from(target_directory.to_string_lossy()))
        .output();

    // println!("{:?}", &output?.stdout);
    // println!("{:?}", &output?.stderr);
    return output;
}

pub async fn fetch_sources(sources: &[Source], work_dir: &PathBuf) -> anyhow::Result<()> {
    let cache_dir = std::env::current_dir()?.join("CACHE");
    fs::create_dir_all(&cache_dir)?;
    println!("Fetching sources");
    for src in sources {
        println!("Checking source: {:?}", src);
        match &src {
            Source::Git(src) => {
                git_src(&src);
            }
            Source::Url(src) => {
                println!("Fetching source! {}", &src.url);
                let res = url_src(&src, &cache_dir).await?;
                validate_checksum(&res, &src.checksum);
                extract(&res, &work_dir).expect("Could not extract the file!");
            }
            _ => {
                println!("Could not match any type!");
            }
        }
    }
    println!("Checking source len: {}", sources.len());
    Ok(())
    // println!("Getting stuff from the WWW");
    // let text = reqwest::get("https://raw.githubusercontent.com/mamba-org/mamba/master/README.md")
    //     .await?
    //     .text()
    //     .await?;
    // println!("body = {:?}", text);
    // Ok(())
}
