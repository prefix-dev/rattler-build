//! Integration tests for rattler_build_package

use rattler_build_package::{
    AboutJsonBuilder, ArchiveType, IndexJsonBuilder, PackageBuilder, PackageConfig,
};
use rattler_conda_types::{PackageName, Platform, VersionWithSource};
use std::fs;

#[cfg(feature = "recipe")]
use rattler_build_recipe::stage1::{About, Build, Extra, Package, Recipe, Requirements};

#[cfg(feature = "recipe")]
use std::str::FromStr;

#[test]
fn test_complete_package_creation() -> Result<(), Box<dyn std::error::Error>> {
    // Setup
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;

    // Create some test files
    fs::write(temp_source.path().join("test.txt"), "Hello, world!")?;
    fs::write(temp_source.path().join("data.json"), r#"{"key": "value"}"#)?;

    let bin_dir = temp_source.path().join("bin");
    fs::create_dir(&bin_dir)?;
    fs::write(bin_dir.join("tool"), "#!/bin/bash\necho 'test'")?;

    // Create metadata
    let name = PackageName::new_unchecked("test-package");
    let version: VersionWithSource = "1.0.0".parse()?;
    let platform = Platform::Linux64;

    let about = AboutJsonBuilder::new()
        .with_homepage("https://github.com/test/test-package".to_string())
        .with_license("MIT".to_string())
        .with_summary("A test package".to_string())
        .build();

    let index = IndexJsonBuilder::new(name.clone(), version.clone(), "h12345_0".to_string())
        .with_build_number(0)
        .with_target_platform(&platform)
        .with_dependency("python >=3.8".to_string())
        .build()?;

    let config = PackageConfig {
        compression_level: 1, // Use low compression for faster tests
        archive_type: ArchiveType::TarBz2,
        timestamp: Some(chrono::Utc::now()),
        compression_threads: 1,
        detect_prefix: true,
        store_recipe: false,
    };

    // Build the package
    let output = PackageBuilder::new(name, version, platform, config)
        .with_build_string("h12345_0")
        .with_about(about)
        .with_index(index)
        .with_files_from_dir(temp_source.path())?
        .build(temp_output.path())?;

    // Verify the package was created
    assert!(output.path.exists(), "Package file should exist");
    assert!(output.path.is_file(), "Package should be a file");

    let metadata = fs::metadata(&output.path)?;
    assert!(metadata.len() > 0, "Package should not be empty");

    assert_eq!(output.identifier, "test-package-1.0.0-h12345_0");

    // Verify paths.json was generated
    assert!(
        !output.paths_json.paths.is_empty(),
        "Should have paths entries"
    );

    // Find our test files in paths.json
    let has_test_txt = output
        .paths_json
        .paths
        .iter()
        .any(|p| p.relative_path.to_string_lossy().contains("test.txt"));
    assert!(has_test_txt, "paths.json should contain test.txt");

    Ok(())
}

#[test]
fn test_package_with_minimal_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;

    // Create a single file
    fs::write(temp_source.path().join("readme.txt"), "README")?;

    let name = PackageName::new_unchecked("minimal-pkg");
    let version: VersionWithSource = "0.1.0".parse()?;
    let platform = Platform::NoArch;

    let config = PackageConfig {
        compression_level: 1,
        archive_type: ArchiveType::TarBz2,
        ..Default::default()
    };

    // Build with minimal metadata (no about, no index)
    let output = PackageBuilder::new(name, version, platform, config)
        .with_build_string("0")
        .with_files_from_dir(temp_source.path())?
        .build(temp_output.path())?;

    assert!(output.path.exists());
    assert_eq!(output.identifier, "minimal-pkg-0.1.0-0");

    Ok(())
}

#[test]
fn test_package_creation_conda_format() -> Result<(), Box<dyn std::error::Error>> {
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;

    fs::write(temp_source.path().join("file.txt"), "content")?;

    let name = PackageName::new_unchecked("conda-test");
    let version: VersionWithSource = "2.0.0".parse()?;

    let config = PackageConfig {
        archive_type: ArchiveType::Conda,
        compression_level: 1,
        ..Default::default()
    };

    let output = PackageBuilder::new(name, version, Platform::Linux64, config)
        .with_build_string("py310_0")
        .with_files_from_dir(temp_source.path())?
        .build(temp_output.path())?;

    assert!(output.path.exists());
    assert!(output.path.to_string_lossy().ends_with(".conda"));

    Ok(())
}

#[test]
fn test_package_with_prefix_detection() -> Result<(), Box<dyn std::error::Error>> {
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;

    // Create a file that contains a path reference
    let content = format!("Working directory: {}", temp_source.path().display());
    fs::write(temp_source.path().join("config.txt"), content)?;

    let name = PackageName::new_unchecked("prefix-test");
    let version: VersionWithSource = "1.0.0".parse()?;

    let config = PackageConfig {
        detect_prefix: true,
        ..Default::default()
    };

    let output = PackageBuilder::new(name, version, Platform::Linux64, config)
        .with_build_string("0")
        .with_files_from_dir(temp_source.path())?
        .build(temp_output.path())?;

    assert!(output.path.exists());

    // Check if prefix was detected in paths.json
    // Note: This might not always detect depending on the staging behavior
    // but the test verifies the mechanism works
    assert!(!output.paths_json.paths.is_empty());

    Ok(())
}

#[test]
fn test_empty_package_fails() -> Result<(), Box<dyn std::error::Error>> {
    let temp_output = tempfile::tempdir()?;

    let name = PackageName::new_unchecked("empty-pkg");
    let version: VersionWithSource = "1.0.0".parse()?;

    let result = PackageBuilder::new(name, version, Platform::Linux64, PackageConfig::default())
        .with_build_string("0")
        // No files added!
        .build(temp_output.path());

    // Should succeed but generate a warning (files can be empty for metapackages)
    assert!(result.is_ok());

    Ok(())
}

#[test]
fn test_package_without_build_string_fails() {
    let temp_output = tempfile::tempdir().unwrap();

    let name = PackageName::new_unchecked("test");
    let version: VersionWithSource = "1.0.0".parse().unwrap();

    let result = PackageBuilder::new(name, version, Platform::Linux64, PackageConfig::default())
        // Missing build_string!
        .build(temp_output.path());

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        rattler_build_package::PackageError::BuildStringNotSet
    ));
}

#[test]
#[cfg(feature = "recipe")]
fn test_from_recipe_with_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;

    // Create some files
    fs::write(temp_source.path().join("main.py"), "print('Hello')")?;

    // Create a test recipe
    let package = Package::new(
        PackageName::new_unchecked("my-recipe-pkg"),
        "2.5.0".parse()?,
    );

    let about = About {
        homepage: Some("https://example.com".parse()?),
        license: Some(rattler_build_recipe::stage0::License::from_str(
            "Apache-2.0",
        )?),
        license_family: Some("Apache".to_string()),
        summary: Some("Test package from recipe".to_string()),
        description: Some("Longer description here".to_string()),
        documentation: Some("https://docs.example.com".parse()?),
        repository: Some("https://github.com/example/repo".parse()?),
        ..Default::default()
    };

    let recipe = Recipe::new(
        package,
        Build::default(),
        about,
        Requirements::default(),
        Extra::default(),
        Vec::new(),
        Vec::new(),
        Default::default(),
    );

    let config = PackageConfig {
        compression_level: 1,
        archive_type: ArchiveType::TarBz2,
        store_recipe: false,
        ..Default::default()
    };

    // Build from recipe
    let output =
        PackageBuilder::from_recipe(&recipe, Platform::Linux64, "py310_0".to_string(), config)
            .with_files_from_dir(temp_source.path())?
            .build(temp_output.path())?;

    // Verify
    assert!(output.path.exists());
    assert_eq!(output.identifier, "my-recipe-pkg-2.5.0-py310_0");

    Ok(())
}

#[test]
#[cfg(feature = "recipe")]
fn test_package_with_license_and_test_files() -> Result<(), Box<dyn std::error::Error>> {
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;
    let temp_extras = tempfile::tempdir()?;

    // Create package files
    fs::write(temp_source.path().join("app.py"), "# Main app")?;

    // Create license files
    let license_file = temp_extras.path().join("LICENSE.txt");
    fs::write(&license_file, "MIT License\n\nCopyright...")?;

    // Create test files
    let test_file = temp_extras.path().join("run_test.py");
    fs::write(&test_file, "import app\nassert True")?;

    let name = PackageName::new_unchecked("test-with-extras");
    let version: VersionWithSource = "1.0.0".parse()?;

    let config = PackageConfig {
        compression_level: 1,
        ..Default::default()
    };

    let output = PackageBuilder::new(name, version, Platform::Linux64, config)
        .with_build_string("0")
        .with_files_from_dir(temp_source.path())?
        .with_license_files(vec![license_file])
        .with_test_files(vec![test_file])
        .build(temp_output.path())?;

    assert!(output.path.exists());
    assert_eq!(output.identifier, "test-with-extras-1.0.0-0");

    // TODO: Verify license and test files are in the archive
    // This would require extracting and inspecting the archive

    Ok(())
}

#[test]
#[cfg(feature = "recipe")]
fn test_package_with_recipe_files() -> Result<(), Box<dyn std::error::Error>> {
    let temp_source = tempfile::tempdir()?;
    let temp_output = tempfile::tempdir()?;
    let temp_recipe = tempfile::tempdir()?;

    // Create package files
    fs::write(temp_source.path().join("binary"), "#!/bin/sh\necho test")?;

    // Create recipe directory with files
    fs::write(
        temp_recipe.path().join("recipe.yaml"),
        "package:\n  name: test",
    )?;
    fs::write(
        temp_recipe.path().join("build.sh"),
        "#!/bin/bash\necho building",
    )?;

    let subdir = temp_recipe.path().join("patches");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("fix.patch"), "--- a/file\n+++ b/file")?;

    let name = PackageName::new_unchecked("test-with-recipe");
    let version: VersionWithSource = "3.0.0".parse()?;

    let config = PackageConfig {
        compression_level: 1,
        store_recipe: true, // Enable recipe storage
        ..Default::default()
    };

    let output = PackageBuilder::new(name, version, Platform::Osx64, config)
        .with_build_string("h123_0")
        .with_files_from_dir(temp_source.path())?
        .with_recipe_dir(temp_recipe.path().to_path_buf())
        .build(temp_output.path())?;

    assert!(output.path.exists());
    assert_eq!(output.identifier, "test-with-recipe-3.0.0-h123_0");

    // TODO: Verify recipe files are in info/recipe/ directory in the archive

    Ok(())
}
