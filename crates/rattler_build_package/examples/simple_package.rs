//! Simple example of creating a conda package

use fs_err as fs;
use rattler_build_package::{
    AboutJsonBuilder, ArchiveType, IndexJsonBuilder, PackageBuilder, PackageConfig,
};
use rattler_conda_types::{PackageName, Platform, VersionWithSource};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory with some files
    let source_dir = tempfile::tempdir()?;
    let output_dir = tempfile::tempdir()?;

    // Create some example files
    fs::write(
        source_dir.path().join("README.md"),
        "# My Package\n\nA simple example package.",
    )?;
    fs::write(source_dir.path().join("data.txt"), "Some data content")?;

    let lib_dir = source_dir.path().join("lib");
    fs::create_dir(&lib_dir)?;
    fs::write(lib_dir.join("example.txt"), "Library file")?;

    // Define package metadata
    let name = PackageName::new_unchecked("my-simple-package");
    let version: VersionWithSource = "1.0.0".parse()?;
    let platform = Platform::Linux64;

    // Create about.json metadata
    let about = AboutJsonBuilder::new()
        .with_homepage("https://github.com/example/my-package".to_string())
        .with_license("MIT".to_string())
        .with_summary("A simple example package".to_string())
        .with_description("This is a demonstration of the rattler_build_package crate".to_string())
        .build();

    // Create index.json metadata
    let index = IndexJsonBuilder::new(name.clone(), version.clone(), "h12345_0".to_string())
        .with_build_number(0)
        .with_target_platform(&platform)
        .with_license("MIT".to_string())
        .with_dependency("python >=3.8".to_string())
        .with_timestamp(chrono::Utc::now())
        .build()?;

    // Configure package creation
    let config = PackageConfig {
        compression_level: 6,
        archive_type: ArchiveType::Conda,
        timestamp: Some(chrono::Utc::now()),
        compression_threads: 4,
        detect_prefix: true,
        store_recipe: false,
    };

    println!("Building package...");

    // Build the package!
    let output = PackageBuilder::new(name, version, platform, config)
        .with_build_string("h12345_0")
        .with_about(about)
        .with_index(index)
        .with_files_from_dir(source_dir.path())?
        .build(output_dir.path())?;

    println!("✅ Package created successfully!");
    println!("   Path: {}", output.path.display());
    println!("   Identifier: {}", output.identifier);
    println!("   Files in package: {}", output.paths_json.paths.len());
    println!("\nPackage contents:");
    for path in &output.paths_json.paths {
        println!("   - {}", path.relative_path.display());
    }

    Ok(())
}
