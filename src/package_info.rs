//! Display information about a built package

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs_err as fs;
use indicatif::HumanBytes;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::package::{
    AboutJson, ArchiveType, IndexJson, PathType, PathsJson, RunExportsJson,
};
use rattler_networking::{AuthenticationMiddleware, AuthenticationStorage};
use rattler_package_streaming::seek::read_package_file;
use reqwest::Client;
use url::Url;

use crate::opt::{ExtractOpts, InspectOpts, PackageSource};

#[cfg(feature = "s3")]
use rattler_networking::s3_middleware;

/// Package metadata read from the archive
struct PackageMetadata {
    index: IndexJson,
    about: Option<AboutJson>,
    paths: Option<PathsJson>,
    run_exports: Option<RunExportsJson>,
}

/// Read and display information about a package
pub fn package_info(args: InspectOpts) -> miette::Result<()> {
    let package_path = &args.package_file;

    // Validate that the path exists and is a file
    if !package_path.exists() {
        return Err(miette::miette!(
            "Package file does not exist: {}",
            package_path.display()
        ));
    }

    if !package_path.is_file() {
        return Err(miette::miette!(
            "Path is not a file: {}. Expected a package file (.conda or .tar.bz2)",
            package_path.display()
        ));
    }

    // Read metadata directly from the package file
    let metadata = read_package_metadata(package_path)?;

    // Output as JSON if requested
    if args.json {
        output_json(
            &metadata.index,
            &metadata.about,
            &metadata.paths,
            &metadata.run_exports,
            &args,
        )?;
        return Ok(());
    }

    // Output human-readable format
    output_human_readable(
        &metadata.index,
        &metadata.about,
        &metadata.paths,
        &metadata.run_exports,
        &args,
        package_path,
    )?;

    Ok(())
}

/// Read package metadata directly from the archive
fn read_package_metadata(package_path: &Path) -> miette::Result<PackageMetadata> {
    // Read index.json (required)
    let index_json: IndexJson = read_package_file(package_path)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Failed to read index.json from package: {}", e))?;

    // Read about.json (optional)
    let about_json: Option<AboutJson> = read_package_file(package_path).ok();

    // Read paths.json (optional)
    let paths_json: Option<PathsJson> = read_package_file(package_path).ok();

    // Read run_exports.json (optional)
    let run_exports_json: Option<RunExportsJson> = read_package_file(package_path).ok();

    Ok(PackageMetadata {
        index: index_json,
        about: about_json,
        paths: paths_json,
        run_exports: run_exports_json,
    })
}

/// Output package information in JSON format
fn output_json(
    index_json: &IndexJson,
    about_json: &Option<AboutJson>,
    paths_json: &Option<PathsJson>,
    run_exports_json: &Option<RunExportsJson>,
    args: &InspectOpts,
) -> miette::Result<()> {
    let mut output = serde_json::Map::new();

    // Always include index info
    output.insert(
        "index".to_string(),
        serde_json::to_value(index_json).into_diagnostic()?,
    );

    // Include about info if requested or available
    if args.show_about()
        && let Some(about) = about_json
    {
        output.insert(
            "about".to_string(),
            serde_json::to_value(about).into_diagnostic()?,
        );
    }

    // Include paths if requested
    if args.show_paths()
        && let Some(paths) = paths_json
    {
        output.insert(
            "paths".to_string(),
            serde_json::to_value(paths).into_diagnostic()?,
        );
    }

    // Include run_exports if requested
    if args.show_run_exports()
        && let Some(run_exports) = run_exports_json
    {
        output.insert(
            "run_exports".to_string(),
            serde_json::to_value(run_exports).into_diagnostic()?,
        );
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&output).into_diagnostic()?
    );

    Ok(())
}

/// Output package information in human-readable format
fn output_human_readable(
    index_json: &IndexJson,
    about_json: &Option<AboutJson>,
    paths_json: &Option<PathsJson>,
    run_exports_json: &Option<RunExportsJson>,
    args: &InspectOpts,
    package_path: &Path,
) -> miette::Result<()> {
    // Package file info
    if package_path.is_file() {
        let size = fs::metadata(package_path).map(|m| m.len()).unwrap_or(0);
        tracing::info!("Package: {} ({})", package_path.display(), HumanBytes(size));
    } else {
        tracing::info!("Package directory: {}", package_path.display());
    }

    // Basic package information
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
        .set_header(vec!["Property", "Value"]);

    table.add_row(vec!["Name", &index_json.name.as_normalized()]);
    table.add_row(vec!["Version", &index_json.version.to_string()]);
    table.add_row(vec!["Build", &index_json.build]);
    table.add_row(vec!["Build number", &index_json.build_number.to_string()]);

    if let Some(ref subdir) = index_json.subdir {
        table.add_row(vec!["Subdir", subdir.as_str()]);
    }

    if let Some(timestamp) = index_json.timestamp {
        // Format timestamp as a readable date (e.g., "2025-11-25 07:56:45 UTC")
        let formatted_time = timestamp
            .datetime()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        table.add_row(vec!["Timestamp", &formatted_time]);
    }

    // Add about.json fields to the main table if available
    if let Some(about) = about_json {
        if let Some(license) = &about.license {
            table.add_row(vec!["License", license]);
        }
        if let Some(ref summary) = about.summary
            && !summary.is_empty()
        {
            table.add_row(vec!["Summary", summary]);
        }
        if let Some(ref description) = about.description
            && !description.is_empty()
        {
            table.add_row(vec!["Description", description]);
        }
        if !about.home.is_empty() {
            let homes = about
                .home
                .iter()
                .map(|h| h.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            table.add_row(vec!["Homepage", &homes]);
        }
        if !about.dev_url.is_empty() {
            let dev_urls = about
                .dev_url
                .iter()
                .map(|u| u.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            table.add_row(vec!["Development URL", &dev_urls]);
        }
        if !about.doc_url.is_empty() {
            let doc_urls = about
                .doc_url
                .iter()
                .map(|u| u.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            table.add_row(vec!["Documentation URL", &doc_urls]);
        }
    }

    tracing::info!("\n{}", table);

    // Dependencies
    if !index_json.depends.is_empty() {
        tracing::info!("\nRun dependencies:");
        let mut dep_table = comfy_table::Table::new();
        dep_table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Package"]);

        for dep in &index_json.depends {
            dep_table.add_row(vec![dep]);
        }

        tracing::info!("{}", dep_table);
    }

    if !index_json.constrains.is_empty() {
        tracing::info!("\nConstraints:");
        let mut constraint_table = comfy_table::Table::new();
        constraint_table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Constraint"]);

        for constraint in &index_json.constrains {
            constraint_table.add_row(vec![constraint]);
        }

        tracing::info!("{}", constraint_table);
    }

    // Paths (only with --paths flag)
    if args.show_paths()
        && let Some(paths) = paths_json
    {
        tracing::info!("\nPackage files ({} total):", paths.paths.len());

        let mut paths_table = comfy_table::Table::new();
        paths_table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Path", "Size", "Type", "Prefix", "SHA256"]);

        for entry in &paths.paths {
            let path_type = match entry.path_type {
                PathType::HardLink => "file",
                PathType::SoftLink => "symlink",
                PathType::Directory => "dir",
            };

            let size = entry
                .size_in_bytes
                .map(|s| HumanBytes(s).to_string())
                .unwrap_or_else(|| "-".to_string());

            let sha256 = entry
                .sha256
                .as_ref()
                .map(hex::encode)
                .unwrap_or_else(|| "-".to_string());

            let prefix_info = if let Some(prefix_placeholder) = &entry.prefix_placeholder {
                match prefix_placeholder.file_mode {
                    rattler_conda_types::package::FileMode::Binary => "binary",
                    rattler_conda_types::package::FileMode::Text => "text",
                }
            } else {
                "-"
            };

            let path = entry.relative_path.to_string_lossy();
            paths_table.add_row(vec![&*path, &size, path_type, prefix_info, &sha256]);
        }

        tracing::info!("{}", paths_table);
    }

    // Run exports (only with --run-exports flag)
    if args.show_run_exports()
        && let Some(run_exports) = run_exports_json
        && !run_exports.is_empty()
    {
        tracing::info!("\nRun exports:");
        let run_exports_str = serde_json::to_string_pretty(run_exports).into_diagnostic()?;
        tracing::info!("{}", run_exports_str);
    }

    Ok(())
}

/// Strips package extensions (.tar.bz2 or .conda) from a filename
fn strip_package_extension(filename: &str) -> &str {
    ArchiveType::split_str(filename)
        .map(|(base, _)| base)
        .unwrap_or(filename)
}

/// Creates an HTTP client with authentication middleware
/// TODO: Refactor to use rattler-build-networking when its available.
fn create_authenticated_client() -> miette::Result<reqwest_middleware::ClientWithMiddleware> {
    let download_client = Client::builder()
        .no_gzip()
        .build()
        .into_diagnostic()
        .context("Failed to create HTTP client")?;

    let authentication_storage =
        AuthenticationStorage::from_env_and_defaults().into_diagnostic()?;

    #[cfg(feature = "s3")]
    let s3_middleware = s3_middleware::S3Middleware::new(
        std::collections::HashMap::new(),
        authentication_storage.clone(),
    );

    let client_builder = reqwest_middleware::ClientBuilder::new(download_client);

    #[cfg(feature = "s3")]
    let client_builder = client_builder.with(s3_middleware);

    let client = client_builder
        .with_arc(Arc::new(AuthenticationMiddleware::from_auth_storage(
            authentication_storage,
        )))
        .with(rattler_networking::OciMiddleware)
        .build();

    Ok(client)
}

/// Determines the destination directory from a URL
fn determine_destination_from_url(url: &Url) -> miette::Result<PathBuf> {
    let filename = url
        .path_segments()
        .and_then(Iterator::last)
        .ok_or_else(|| miette::miette!("Could not extract package name from URL"))?;

    let package_name = strip_package_extension(filename);
    Ok(PathBuf::from(package_name))
}

/// Determines the destination directory from a file path
fn determine_destination_from_path(package_path: &Path) -> miette::Result<PathBuf> {
    let filename = package_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| miette::miette!("Invalid package filename"))?;

    let package_name = strip_package_extension(filename);
    Ok(PathBuf::from(package_name))
}

/// Extracts a conda package from a URL
async fn extract_from_url(
    url: Url,
    destination: Option<PathBuf>,
    package_display: &str,
) -> miette::Result<(PathBuf, rattler_package_streaming::ExtractResult)> {
    let destination = destination.map_or_else(|| determine_destination_from_url(&url), Ok)?;

    println!(
        "Extracting {} to {}",
        package_display,
        destination.display()
    );

    let client = create_authenticated_client()?;

    let result =
        rattler_package_streaming::reqwest::tokio::extract(client, url, &destination, None, None)
            .await
            .into_diagnostic()
            .with_context(|| format!("Failed to extract package from URL: {package_display}"))?;

    Ok((destination, result))
}

/// Extracts a conda package from a local file path
fn extract_from_path(
    package_path: &Path,
    destination: Option<PathBuf>,
) -> miette::Result<(PathBuf, rattler_package_streaming::ExtractResult)> {
    let destination =
        destination.map_or_else(|| determine_destination_from_path(package_path), Ok)?;

    println!(
        "Extracting {} to {}",
        package_path.display(),
        destination.display()
    );

    let result = rattler_package_streaming::fs::extract(package_path, &destination)
        .into_diagnostic()
        .with_context(|| format!("Failed to extract package: {}", package_path.display()))?;

    Ok((destination, result))
}

/// Extract a conda package to a directory
pub async fn extract_package(args: ExtractOpts) -> miette::Result<()> {
    let (destination, result) = match &args.package_file {
        PackageSource::Url(url) => {
            let url_str = url.to_string();
            extract_from_url(url.clone(), args.dest.clone(), &url_str).await?
        }
        PackageSource::Path(path) => extract_from_path(path, args.dest.clone())?,
    };

    println!(
        "{} Successfully extracted package",
        console::style("✔").green(),
    );
    println!("  Destination: {}", destination.display());
    println!("  SHA256: {:x}", result.sha256);
    println!("  MD5: {:x}", result.md5);
    println!(
        "  Size: {} ({} bytes)",
        HumanBytes(result.total_size),
        result.total_size
    );

    Ok(())
}
