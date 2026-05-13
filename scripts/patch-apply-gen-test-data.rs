// Release build is required. See https://github.com/seanmonstar/reqwest/issues/1017.
use std::{
    collections::HashMap,
    default::Default,
    path::{Path, PathBuf},
};

use async_compression::tokio::bufread::ZstdDecoder;
use async_http_range_reader::{AsyncHttpRangeReader, CheckSupportMethod};
use async_zip::base::read::seek::ZipFileReader;
use futures::StreamExt;
use http::HeaderMap;
use miette::{Context as _, IntoDiagnostic};
use rattler_conda_types::{Channel, MatchSpec, ParseStrictness, Platform, RepoDataRecord};
use rattler_networking::LazyClient;
use rattler_repodata_gateway::{
    Gateway, fetch,
    sparse::{PackageFormatSelection, SparseRepoData},
};
use reqwest_middleware::ClientWithMiddleware;
use tokio::io::AsyncReadExt;
use tokio::task::JoinSet;
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use url::Url;

const OUTPUT_PATH: &str = "test-data/conda_forge/recipes/";

// 64KB should be enough for most packages to include the EOCD, Central Directory,
// and often the entire info archive.
const DEFAULT_TAIL_SIZE: u64 = 64 * 1024;

// Overview:
// 1. Get repodata for conda-forge linux-64.
// 2. Get all package names.
// 3. Get package records and filter them to keep only latest versions.
// 4. For each record try to find recipe.yaml and at least one patch.
// 5. Save corresponding files to the `./recipes` directory if some found.
//
// Record processed in parallel, and when record matches criteria we
// write success message.
//
// Note that since function is executed asynchronously, you will see
// results only at the end of processing. On my machine it takes
// around 40 minutes.
#[tokio::main]
async fn main() {
    let repodata_url = Url::parse("https://conda.anaconda.org/conda-forge/linux-64/").unwrap();
    let client = LazyClient::default();
    let cache = PathBuf::from("./cache");

    let result = fetch::fetch_repo_data(
        repodata_url.clone(),
        client.clone(),
        cache.clone(),
        fetch::FetchRepoDataOptions {
            ..Default::default()
        },
        None,
    )
    .await
    .unwrap();

    let repo_path = result.repo_data_json_path.clone();

    let channel = Channel::from_url(Url::parse("https://conda.anaconda.org/conda-forge/").unwrap());
    let platform = Platform::Linux64;

    let repo_data =
        SparseRepoData::from_file(channel.clone(), "linux-64".to_string(), repo_path, None)
            .unwrap();
    let package_names = repo_data
        .package_names(PackageFormatSelection::default())
        .map(|n| MatchSpec::from_str(n, ParseStrictness::Lenient).unwrap());

    let http_client = client.client().clone();
    let gateway = Gateway::builder().with_client(client).finish();

    let repo_data = gateway
        .query(vec![channel], vec![platform], package_names)
        .await
        .unwrap()
        .into_iter()
        .next() // Expect repodata for only one platform.
        .unwrap();

    let mut latest_records = HashMap::new();

    for record in repo_data.iter() {
        let rec = latest_records
            .entry(record.package_record.name.clone())
            .or_insert(record.clone()); // record.package_record.name()
        if rec.package_record.version < record.package_record.version {
            *rec = record.clone();
        }
    }

    let latest_records = latest_records.values().cloned().collect::<Vec<_>>();
    let total = latest_records.len();

    let mut record_tasks: JoinSet<miette::Result<String>> = JoinSet::new();
    for record in latest_records.into_iter() {
        let recipe_expected_path = PathBuf::from("recipe.yaml");

        record_tasks.spawn(handle_record(
            record,
            recipe_expected_path,
            http_client.clone(),
        ));
    }

    let mut successes = 0;
    let mut failures = 0;
    while let Some(res) = record_tasks.join_next().await {
        if let Ok(Ok(pkg_name)) = res {
            println!("Successfully dealt with {}", pkg_name);
            successes += 1;
        } else {
            println!("Error: {:#?}", res);
            failures += 1;
        }
        println!("Processed {}/{}/{}", successes, failures, total);
    }
}

/// Stream the `info/` section of a `.conda` archive over HTTP range requests
/// and return the contents of `info/recipe/recipe.yaml` along with any
/// `info/recipe/*.patch` files. Returns `Ok(None)` if no recipe.yaml is found.
async fn fetch_recipe_files_sparse(
    client: ClientWithMiddleware,
    url: Url,
    recipe_filename: &Path,
) -> miette::Result<Option<(Vec<u8>, Vec<(PathBuf, Vec<u8>)>)>> {
    let (reader, _) = AsyncHttpRangeReader::new(
        client,
        url,
        CheckSupportMethod::NegativeRangeRequest(DEFAULT_TAIL_SIZE),
        HeaderMap::default(),
    )
    .await
    .into_diagnostic()?;

    let buf_reader = futures::io::BufReader::new(reader.compat());
    let mut zip_reader = ZipFileReader::new(buf_reader).await.into_diagnostic()?;

    let (index, _) = zip_reader
        .file()
        .entries()
        .iter()
        .enumerate()
        .find(|(_, e)| {
            e.filename()
                .as_str()
                .is_ok_and(|f| f.starts_with("info-") && f.ends_with(".tar.zst"))
        })
        .ok_or_else(|| miette::miette!("no info-*.tar.zst entry in archive"))?;

    // Prefetch the entire info entry in a single HTTP request.
    let entry = &zip_reader.file().entries()[index];
    let offset = entry.header_offset();
    let size = entry.header_size() + entry.compressed_size();
    zip_reader
        .inner_mut()
        .get_mut()
        .get_mut()
        .prefetch(offset..offset + size)
        .await;

    let entry_reader = zip_reader
        .reader_without_entry(index)
        .await
        .into_diagnostic()?;
    let tokio_reader = entry_reader.compat();
    let buf_reader = tokio::io::BufReader::new(tokio_reader);
    let zstd_decoder = ZstdDecoder::new(buf_reader);
    let mut tar = tokio_tar::Archive::new(zstd_decoder);

    let mut entries = tar.entries().into_diagnostic()?;
    let mut recipe = None;
    let mut patches = Vec::new();
    while let Some(entry) = entries.next().await {
        let mut entry = entry.into_diagnostic()?;
        let path = entry.path().into_diagnostic()?.into_owned();
        let Ok(rel) = path.strip_prefix("info/recipe") else {
            continue;
        };

        if rel == recipe_filename {
            let size = entry.header().size().into_diagnostic()?;
            let mut buf = Vec::with_capacity(size as usize);
            entry.read_to_end(&mut buf).await.into_diagnostic()?;
            recipe = Some(buf);
        } else if rel.extension().and_then(|s| s.to_str()) == Some("patch") {
            let size = entry.header().size().into_diagnostic()?;
            let mut buf = Vec::with_capacity(size as usize);
            entry.read_to_end(&mut buf).await.into_diagnostic()?;
            patches.push((rel.to_path_buf(), buf));
        }
    }

    Ok(recipe.map(|r| (r, patches)))
}

async fn handle_record(
    record: RepoDataRecord,
    recipe_expected_path: PathBuf,
    client: ClientWithMiddleware,
) -> miette::Result<String> {
    let pkg_name = record.package_record.name.as_source();

    let result = fetch_recipe_files_sparse(client, record.url.clone(), &recipe_expected_path)
        .await
        .with_context(|| format!("{}: failed to stream conda info section", pkg_name))?;

    let Some((recipe_bytes, patch_entries)) = result else {
        return Err(miette::miette!(
            "{}: no info/recipe/{} found",
            pkg_name,
            recipe_expected_path.display()
        ));
    };

    if patch_entries.is_empty() {
        return Err(miette::miette!(
            "{}: no patch files found alongside recipe.yaml",
            pkg_name
        ));
    }

    let package_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(OUTPUT_PATH)
        .join(pkg_name);

    let files = std::iter::once((recipe_expected_path.clone(), recipe_bytes)).chain(patch_entries);

    let mut any_failed = None;
    for (path, content) in files {
        let file_new_path = package_path.join(&path);

        if let Err(e) = tokio::fs::create_dir_all(file_new_path.parent().unwrap()).await {
            any_failed = Some(
                Err(e)
                    .into_diagnostic()
                    .with_context(|| format!("{}: error creating dir", pkg_name)),
            );
            break;
        };

        if let Err(e) = tokio::fs::write(file_new_path, content).await {
            any_failed = Some(
                Err(e)
                    .into_diagnostic()
                    .with_context(|| format!("{}: error writing file to dir", pkg_name)),
            );
            break;
        };
    }

    if let Some(e) = any_failed {
        tokio::fs::remove_dir_all(package_path)
            .await
            .into_diagnostic()
            .with_context(|| {
                format!(
                    "{}: issue occurred when tried to remove package directory after getting {:#?}",
                    pkg_name, e
                )
            })?;
        return e;
    }

    Ok(pkg_name.to_string())
}
