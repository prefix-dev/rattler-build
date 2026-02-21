use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "cli")]
use clap::Parser;
use miette::IntoDiagnostic;

use crate::pypi::{conda_pypi_name_mapping, format_requirement, map_requirement};
use crate::serialize::{self, PythonTest, PythonTestInner, Test, UrlSourceElement};
use crate::write_recipe;

/// Options for generating a recipe from a local `pyproject.toml`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "cli", derive(Parser))]
pub struct PyprojectOpts {
    /// Path to the pyproject.toml file (defaults to ./pyproject.toml)
    #[cfg_attr(feature = "cli", arg(default_value = "pyproject.toml"))]
    pub path: PathBuf,

    /// Whether to write the recipe to a folder
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub write: bool,

    /// Whether to use the conda-forge PyPI name mapping
    #[cfg_attr(feature = "cli", arg(short, long, default_value = "true"))]
    pub use_mapping: bool,
}

/// Parsed content from a pyproject.toml file.
#[derive(Debug)]
struct PyprojectMetadata {
    name: String,
    version: String,
    description: Option<String>,
    license: Option<String>,
    license_expression: Option<String>,
    requires_python: Option<String>,
    dependencies: Vec<String>,
    build_requires: Vec<String>,
    scripts: Vec<String>,
    urls: HashMap<String, String>,
}

fn parse_pyproject(contents: &str) -> miette::Result<PyprojectMetadata> {
    let toml: toml::Table = contents.parse().into_diagnostic()?;

    let project = toml
        .get("project")
        .and_then(|v| v.as_table())
        .ok_or_else(|| miette::miette!("pyproject.toml is missing [project] table"))?;

    let name = project
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| miette::miette!("pyproject.toml [project] is missing 'name'"))?
        .to_string();

    let version = project
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let description = project
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // License: prefer license-expression (PEP 639), then license as a table with
    // `text` key, then license as a plain string.
    let license_expression = project
        .get("license-expression")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let license = if license_expression.is_some() {
        None
    } else {
        project.get("license").and_then(|v| {
            // PEP 639: license can be a string (SPDX expression) or a table with `text`/`file`.
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if let Some(table) = v.as_table() {
                table.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        })
    };

    let requires_python = project
        .get("requires-python")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let dependencies = project
        .get("dependencies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let build_requires = toml
        .get("build-system")
        .and_then(|v| v.get("requires"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    // Extract [project.scripts] for console entry points.
    let scripts = project
        .get("scripts")
        .and_then(|v| v.as_table())
        .map(|table| {
            table
                .iter()
                .map(|(name, val)| {
                    format!("{} = {}", name, val.as_str().unwrap_or_default())
                })
                .collect()
        })
        .unwrap_or_default();

    // Extract [project.urls]
    let urls = project
        .get("urls")
        .and_then(|v| v.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    Ok(PyprojectMetadata {
        name,
        version,
        description,
        license,
        license_expression,
        requires_python,
        dependencies,
        build_requires,
        scripts,
        urls,
    })
}

/// Create a `serialize::Recipe` from a parsed `pyproject.toml`.
async fn create_recipe_from_pyproject(
    metadata: &PyprojectMetadata,
    use_mapping: bool,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();

    recipe
        .context
        .insert("version".to_string(), metadata.version.clone());
    recipe
        .context
        .insert("build_number".to_string(), "0".to_string());
    recipe.package.name = metadata.name.to_lowercase();
    recipe.package.version = "${{ version }}".to_string();
    recipe.build.number = "${{ build_number }}".to_string();
    recipe.build.noarch = Some("python".to_string());

    // Use a path source pointing to the project root.
    // The source section is left empty — the user is expected to fill in the
    // URL/git/path source depending on how they want to distribute.
    // We add a placeholder URL source that they can replace.
    recipe.source.push(
        UrlSourceElement {
            url: vec!["# TODO: set the source url".to_string()],
            sha256: Some("# TODO: set the sha256 hash".to_string()),
            md5: None,
        }
        .into(),
    );

    // Set Python requirements
    if let Some(python_req) = &metadata.requires_python {
        recipe
            .requirements
            .host
            .push(format!("python {}", python_req));
        recipe
            .requirements
            .run
            .push(format!("python {}", python_req));
    } else {
        recipe.requirements.host.push("python".to_string());
    }

    let mapping = if use_mapping {
        conda_pypi_name_mapping().await?
    } else {
        &HashMap::new()
    };

    // Build/host requirements from [build-system].requires
    for req in &metadata.build_requires {
        let mapped = map_requirement(req, mapping, use_mapping).await;
        recipe.requirements.host.push(mapped);
    }
    recipe.requirements.host.push("pip".to_string());

    // Runtime dependencies from [project].dependencies
    for req in &metadata.dependencies {
        let mapped = map_requirement(req, mapping, use_mapping).await;
        let formatted = format_requirement(&mapped);
        recipe
            .requirements
            .run
            .push(formatted.trim_start_matches("- ").to_string());
    }

    // Entry points from [project.scripts]
    if !metadata.scripts.is_empty() {
        recipe.build.python.entry_points = metadata.scripts.clone();
    }

    recipe.build.script = "${{ PYTHON }} -m pip install .".to_string();

    // Tests: import the package and run pip check.
    recipe.tests.push(Test::Python(PythonTest {
        python: PythonTestInner {
            imports: vec![metadata.name.replace('-', "_")],
            pip_check: true,
        },
    }));

    // About section
    recipe.about.summary = metadata.description.clone();

    if let Some(expr) = &metadata.license_expression {
        recipe.about.license = Some(expr.clone());
    } else if let Some(lic) = &metadata.license {
        recipe.about.license = Some(lic.clone());
    }

    // Try common URL key names
    recipe.about.homepage = metadata
        .urls
        .get("Homepage")
        .or_else(|| metadata.urls.get("homepage"))
        .cloned();
    recipe.about.repository = metadata
        .urls
        .get("Source Code")
        .or_else(|| metadata.urls.get("Source")
            .or_else(|| metadata.urls.get("source"))
            .or_else(|| metadata.urls.get("Repository"))
            .or_else(|| metadata.urls.get("repository")))
        .cloned();
    recipe.about.documentation = metadata
        .urls
        .get("Documentation")
        .or_else(|| metadata.urls.get("documentation"))
        .or_else(|| metadata.urls.get("Docs"))
        .cloned();

    Ok(recipe)
}

/// Generate a recipe YAML string from a local `pyproject.toml`.
pub async fn generate_pyproject_recipe_string(opts: &PyprojectOpts) -> miette::Result<String> {
    let contents = fs_err::read_to_string(&opts.path)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Failed to read {}: {}", opts.path.display(), e))?;
    let metadata = parse_pyproject(&contents)?;
    let recipe = create_recipe_from_pyproject(&metadata, opts.use_mapping).await?;
    Ok(format!("{}", recipe))
}

/// Generate a recipe from a local `pyproject.toml` and either write it to disk or print it.
pub async fn generate_pyproject_recipe(opts: &PyprojectOpts) -> miette::Result<()> {
    let contents = fs_err::read_to_string(&opts.path)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Failed to read {}: {}", opts.path.display(), e))?;
    let metadata = parse_pyproject(&contents)?;

    tracing::info!(
        "Generating recipe from {} for package '{}'",
        opts.path.display(),
        metadata.name
    );

    let recipe = create_recipe_from_pyproject(&metadata, opts.use_mapping).await?;
    let string = format!("{}", recipe);

    if opts.write {
        write_recipe(&metadata.name, &string).into_diagnostic()?;
    } else {
        print!("{}", string);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_pyproject() {
        let toml = r#"
[project]
name = "my-package"
version = "1.2.3"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(meta.name, "my-package");
        assert_eq!(meta.version, "1.2.3");
        assert!(meta.dependencies.is_empty());
        assert!(meta.build_requires.is_empty());
    }

    #[test]
    fn test_parse_full_pyproject() {
        let toml = r#"
[build-system]
requires = ["setuptools>=64", "wheel"]

[project]
name = "my-package"
version = "0.5.0"
description = "A cool package"
requires-python = ">=3.9"
license = "MIT"
dependencies = [
    "requests>=2.20",
    "click",
]

[project.scripts]
my-cli = "my_package.cli:main"

[project.urls]
Homepage = "https://example.com"
"Source Code" = "https://github.com/example/my-package"
Documentation = "https://my-package.readthedocs.io"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(meta.name, "my-package");
        assert_eq!(meta.version, "0.5.0");
        assert_eq!(meta.description.as_deref(), Some("A cool package"));
        assert_eq!(meta.license.as_deref(), Some("MIT"));
        assert_eq!(meta.requires_python.as_deref(), Some(">=3.9"));
        assert_eq!(meta.dependencies, vec!["requests>=2.20", "click"]);
        assert_eq!(
            meta.build_requires,
            vec!["setuptools>=64", "wheel"]
        );
        assert_eq!(meta.scripts, vec!["my-cli = my_package.cli:main"]);
        assert_eq!(
            meta.urls.get("Homepage").unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn test_parse_license_expression() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"
license-expression = "Apache-2.0 OR MIT"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.license_expression.as_deref(),
            Some("Apache-2.0 OR MIT")
        );
        assert!(meta.license.is_none());
    }

    #[test]
    fn test_parse_license_table() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[project.license]
text = "BSD-3-Clause"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(meta.license.as_deref(), Some("BSD-3-Clause"));
    }

    #[test]
    fn test_missing_project_table() {
        let toml = r#"
[build-system]
requires = ["setuptools"]
"#;
        let result = parse_pyproject(toml);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_recipe_from_pyproject() {
        let meta = PyprojectMetadata {
            name: "my-package".into(),
            version: "1.0.0".into(),
            description: Some("A description".into()),
            license: Some("MIT".into()),
            license_expression: None,
            requires_python: Some(">=3.9".into()),
            dependencies: vec!["requests>=2.20".into(), "click".into()],
            build_requires: vec!["setuptools>=64".into()],
            scripts: vec!["my-cli = my_package.cli:main".into()],
            urls: HashMap::from([
                ("Homepage".into(), "https://example.com".into()),
            ]),
        };

        let recipe = create_recipe_from_pyproject(&meta, false).await.unwrap();
        assert_eq!(recipe.package.name, "my-package");
        assert_eq!(recipe.package.version, "${{ version }}");
        assert_eq!(recipe.about.license, Some("MIT".into()));
        assert_eq!(recipe.about.homepage, Some("https://example.com".into()));
        assert_eq!(recipe.build.python.entry_points, vec!["my-cli = my_package.cli:main"]);
        assert!(recipe.requirements.host.contains(&"python >=3.9".to_string()));
        assert!(recipe.requirements.run.contains(&"python >=3.9".to_string()));
    }
}
