// Release build is required. See https://github.com/seanmonstar/reqwest/issues/1017.
use std::{
    collections::HashMap,
    default::Default,
    io::{BufReader, Cursor, Read},
    path::{Path, PathBuf},
};

use miette::{Context as _, IntoDiagnostic};
use rattler_conda_types::{Channel, MatchSpec, ParseStrictness, Platform, RepoDataRecord};
use rattler_networking::LazyClient;
use rattler_package_streaming::{
    reqwest::sparse::fetch_file_from_remote_sparse, seek::stream_conda_info,
};
use rattler_repodata_gateway::{
    Gateway, fetch,
    sparse::{PackageFormatSelection, SparseRepoData},
};
use reqwest_middleware::ClientWithMiddleware;

use tokio::task::JoinSet;
use url::Url;

const OUTPUT_PATH: &str = "test-data/conda_forge/recipes/";

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

async fn handle_record(
    record: RepoDataRecord,
    recipe_expected_path: PathBuf,
    client: ClientWithMiddleware,
) -> miette::Result<String> {
    let pkg_name = record.package_record.name.as_source();

    // Cheap gating check: use HTTP range requests to peek at info/recipe/recipe.yaml.
    // Most packages don't have one, so we skip them without downloading the whole archive.
    let target = Path::new("info").join("recipe").join(&recipe_expected_path);
    let recipe_bytes = fetch_file_from_remote_sparse(client.clone(), record.url.clone(), &target)
        .await
        .into_diagnostic()
        .with_context(|| format!("{}: sparse fetch failed", pkg_name))?;

    if recipe_bytes.is_none() {
        return Err(miette::miette!(
            "{}: no info/recipe/{} found",
            pkg_name,
            recipe_expected_path.display()
        ));
    }

    // The package has a recipe — fetch the full archive to also enumerate patches.
    let response = client
        .get(record.url.clone())
        .send()
        .await
        .into_diagnostic()
        .with_context(|| format!("{}: failed to download package", pkg_name))?
        .error_for_status()
        .into_diagnostic()?;
    let bytes = response
        .bytes()
        .await
        .into_diagnostic()
        .with_context(|| format!("{}: failed to read package body", pkg_name))?;

    let mut sci = stream_conda_info(Cursor::new(bytes))
        .into_diagnostic()
        .with_context(|| format!("{}: can't stream conda info", pkg_name))?;

    let entries = sci
        .entries()
        .into_diagnostic()
        .with_context(|| format!("{}: could not obtain entries", pkg_name))?;

    let entries = entries.filter_map(|entry| entry.ok());

    let mut recipe_entry = None;
    let mut patch_entries = vec![];
    for entry in entries.into_iter() {
        let Ok(path) = entry.path() else {
            continue;
        };
        let Ok(path) = path.strip_prefix("info/recipe") else {
            continue;
        };

        if path == recipe_expected_path.clone().as_path() {
            let path = path.to_path_buf();
            let mut reader = BufReader::new(entry);
            let mut content = String::new();
            reader
                .read_to_string(&mut content)
                .into_diagnostic()
                .with_context(|| format!("{}: problem reading recipe.yaml", pkg_name))?;
            recipe_entry = Some((path, content));
        } else if path.extension().and_then(|s| s.to_str()) == Some("patch") {
            let path = path.to_path_buf();
            let mut reader = BufReader::new(entry);
            let mut content = String::new();
            reader
                .read_to_string(&mut content)
                .into_diagnostic()
                .with_context(|| format!("{}: problem reading patch file.", pkg_name))?;
            patch_entries.push((path, content));
        }
    }

    if recipe_entry.is_none() || patch_entries.is_empty() {
        return Err(tokio::io::Error::new(
            tokio::io::ErrorKind::NotFound,
            "Could not find recipe.yaml and patch files",
        ))
        .into_diagnostic()
        .with_context(|| pkg_name.to_string());
    }

    let mut any_failed = None;
    let package_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(OUTPUT_PATH)
        .join(pkg_name);

    for (path, content) in recipe_entry.into_iter().chain(patch_entries.into_iter()) {
        let file_new_path = package_path.join(path);

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
