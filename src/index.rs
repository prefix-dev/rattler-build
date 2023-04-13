use anyhow::Result;
use rattler_conda_types::package::ArchiveType;
use rattler_conda_types::package::IndexJson;
use rattler_conda_types::package::PackageFile;
use rattler_conda_types::ChannelInfo;
use rattler_conda_types::PackageRecord;
use rattler_conda_types::Platform;
use rattler_conda_types::RepoData;
use rattler_package_streaming::read;
use rattler_package_streaming::seek;

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;

fn package_record_from_index_json<T: Read>(
    file: &Path,
    index_json_reader: &mut T,
) -> Result<PackageRecord, std::io::Error> {
    let index = IndexJson::from_reader(index_json_reader).unwrap();

    let sha256_result =
        rattler_digest::compute_file_digest::<rattler_digest::Sha256>(file).unwrap();
    let md5_result = rattler_digest::compute_file_digest::<rattler_digest::Md5>(file).unwrap();
    let size = std::fs::metadata(file).unwrap().len();

    let package_record = PackageRecord {
        name: index.name,
        version: index.version,
        build: index.build,
        build_number: index.build_number,
        subdir: index.subdir.unwrap_or_else(|| "unknown".to_string()),
        md5: Some(md5_result),
        sha256: Some(sha256_result),
        size: Some(size),
        arch: index.arch,
        platform: index.platform,
        depends: index.depends,
        constrains: index.constrains,
        track_features: index.track_features,
        features: index.features,
        noarch: index.noarch,
        license: index.license,
        license_family: index.license_family,
        timestamp: index.timestamp,
        legacy_bz2_md5: None,
        legacy_bz2_size: None,
    };
    Ok(package_record)
}

fn package_record_from_tar_bz2(file: &Path) -> Result<PackageRecord, std::io::Error> {
    let reader = std::fs::File::open(file).unwrap();
    let mut archive = read::stream_tar_bz2(reader);

    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap();
        let path = path.to_str().unwrap();
        if path == "info/index.json" {
            return package_record_from_index_json(file, &mut entry);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "No index.json found",
    ))
}

fn package_record_from_conda(file: &Path) -> Result<PackageRecord, std::io::Error> {
    let reader = std::fs::File::open(file).unwrap();
    let mut archive = seek::stream_conda_info(reader).expect("Could not open conda file");

    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap();
        let path = path.to_str().unwrap();
        if path == "info/index.json" {
            return package_record_from_index_json(file, &mut entry);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "No index.json found",
    ))
}

/// Create a new `repodata.json` for all packages in the given output folder. If `target_platform` is
/// `Some`, only that specific subdir is indexed. Otherwise indexes all subdirs and creates a
/// `repodata.json` for each.
pub fn index(output_folder: &Path, target_platform: Option<&Platform>) -> Result<()> {
    let entries = WalkDir::new(output_folder).into_iter();
    let entries: Vec<(PathBuf, ArchiveType)> = entries
        .filter_entry(|e| e.depth() <= 2)
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            ArchiveType::split_str(e.path().to_string_lossy().as_ref())
                .map(|(p, t)| (PathBuf::from(format!("{}{}", p, t.extension())), t))
        })
        .collect();

    // find all subdirs
    let platforms = entries
        .iter()
        .map(|(p, _)| {
            p.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect::<std::collections::HashSet<_>>();

    for platform in platforms {
        if let Some(target_platform) = target_platform {
            if platform != target_platform.to_string() {
                if platform != "noarch" {
                    continue;
                } else {
                    // check that noarch is already indexed if it is not the target platform
                    if output_folder.join("noarch/repodata.json").exists() {
                        continue;
                    }
                }
            }
        }

        let mut repodata = RepoData {
            info: Some(ChannelInfo {
                subdir: platform.clone(),
            }),
            packages: Default::default(),
            conda_packages: Default::default(),
            removed: Default::default(),
            version: Some(1),
        };

        for (p, t) in entries
            .iter()
            .filter(|(p, _)| p.parent().unwrap().file_name().unwrap() == OsStr::new(&platform))
        {
            match t {
                ArchiveType::TarBz2 => {
                    if let Ok(record) = package_record_from_tar_bz2(p) {
                        repodata
                            .packages
                            .insert(p.file_name().unwrap().to_string_lossy().into(), record);
                    } else {
                        println!("Could not read package record from {:?}", p);
                    }
                }
                ArchiveType::Conda => {
                    if let Ok(record) = package_record_from_conda(p) {
                        repodata
                            .conda_packages
                            .insert(p.file_name().unwrap().to_string_lossy().to_string(), record);
                    } else {
                        println!("Could not read package record from {:?}", p);
                    }
                }
            };
        }
        let out_file = output_folder.join(platform).join("repodata.json");
        File::create(&out_file)
            .unwrap()
            .write_all(serde_json::to_string_pretty(&repodata).unwrap().as_bytes())
            .unwrap();
    }

    Ok(())
}

#[cfg(test)]
mod test {

    #[test]
    fn test_index() {
        // let output_folder = PathBuf::from("/Users/wolfv/Programs/roar/output");
        // let target_platform = "osx-64";
        // index(&output_folder, Some(target_platform)).unwrap();
    }
}
