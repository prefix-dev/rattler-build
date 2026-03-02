use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "cli")]
use clap::Parser;
use miette::IntoDiagnostic;

use crate::pypi::{conda_pypi_name_mapping, format_requirement, map_requirement};
use crate::serialize::{
    self, GitSourceElement, PythonTest, PythonTestInner, ScriptTest, Test, UrlSourceElement,
};
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
    /// Overrides from `[tool.rattler-build]`.
    overrides: RecipeOverrides,
}

/// Overrides that users can specify in `[tool.rattler-build]` to customise the
/// generated recipe.  Every field is optional — only what the user provides is
/// applied on top of the auto-generated recipe.
#[derive(Debug, Default)]
struct RecipeOverrides {
    // context
    context: HashMap<String, String>,
    // package
    package_name: Option<String>,
    // source
    source_url: Option<String>,
    source_sha256: Option<String>,
    source_git: Option<String>,
    source_tag: Option<String>,
    source_branch: Option<String>,
    // build
    script: Option<String>,
    noarch: Option<String>,
    // requirements
    build_reqs: Option<Vec<String>>,
    host_reqs: Option<Vec<String>>,
    run_reqs: Option<Vec<String>>,
    // tests
    test_imports: Option<Vec<String>>,
    test_commands: Option<Vec<String>>,
    // about
    homepage: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    license: Option<String>,
    license_file: Option<Vec<String>>,
    repository: Option<String>,
    documentation: Option<String>,
}

fn parse_string_array(val: &toml::Value) -> Option<Vec<String>> {
    val.as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect()
    })
}

/// Parse `[tool.rattler-build]` overrides from the TOML table.
fn parse_overrides(toml: &toml::Table) -> RecipeOverrides {
    let rb = toml
        .get("tool")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("rattler-build"))
        .and_then(|v| v.as_table());

    let rb = match rb {
        Some(t) => t,
        None => return RecipeOverrides::default(),
    };

    let mut ov = RecipeOverrides::default();

    // [tool.rattler-build.context]
    if let Some(ctx) = rb.get("context").and_then(|v| v.as_table()) {
        for (k, v) in ctx {
            if let Some(s) = v.as_str() {
                ov.context.insert(k.clone(), s.to_string());
            }
        }
    }

    // [tool.rattler-build.package]
    if let Some(pkg) = rb.get("package").and_then(|v| v.as_table()) {
        ov.package_name = pkg.get("name").and_then(|v| v.as_str()).map(|s| s.into());
    }

    // [tool.rattler-build.source]
    if let Some(src) = rb.get("source").and_then(|v| v.as_table()) {
        ov.source_url = src.get("url").and_then(|v| v.as_str()).map(|s| s.into());
        ov.source_sha256 = src.get("sha256").and_then(|v| v.as_str()).map(|s| s.into());
        ov.source_git = src.get("git").and_then(|v| v.as_str()).map(|s| s.into());
        ov.source_tag = src.get("tag").and_then(|v| v.as_str()).map(|s| s.into());
        ov.source_branch = src
            .get("branch")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
    }

    // [tool.rattler-build.build]
    if let Some(bld) = rb.get("build").and_then(|v| v.as_table()) {
        ov.script = bld.get("script").and_then(|v| v.as_str()).map(|s| s.into());
        ov.noarch = bld.get("noarch").and_then(|v| v.as_str()).map(|s| s.into());
    }

    // [tool.rattler-build.requirements]
    if let Some(reqs) = rb.get("requirements").and_then(|v| v.as_table()) {
        ov.build_reqs = reqs.get("build").and_then(parse_string_array);
        ov.host_reqs = reqs.get("host").and_then(parse_string_array);
        ov.run_reqs = reqs.get("run").and_then(parse_string_array);
    }

    // [tool.rattler-build.tests]
    if let Some(tests) = rb.get("tests").and_then(|v| v.as_table()) {
        ov.test_imports = tests.get("imports").and_then(parse_string_array);
        ov.test_commands = tests.get("commands").and_then(parse_string_array);
    }

    // [tool.rattler-build.about]
    if let Some(about) = rb.get("about").and_then(|v| v.as_table()) {
        ov.homepage = about
            .get("homepage")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
        ov.summary = about
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
        ov.description = about
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
        ov.license = about
            .get("license")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
        ov.license_file = about.get("license_file").and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(vec![s.to_string()])
            } else {
                parse_string_array(v)
            }
        });
        ov.repository = about
            .get("repository")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
        ov.documentation = about
            .get("documentation")
            .and_then(|v| v.as_str())
            .map(|s| s.into());
    }

    ov
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

    // Version: static field, or resolve from dynamic declaration.
    let version = if let Some(v) = project.get("version").and_then(|v| v.as_str()) {
        v.to_string()
    } else {
        let is_dynamic = project
            .get("dynamic")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(|v| v.as_str() == Some("version")))
            .unwrap_or(false);

        if is_dynamic {
            resolve_dynamic_version(&toml)
        } else {
            "0.0.0".to_string()
        }
    };

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
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if let Some(table) = v.as_table() {
                table
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
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

    let overrides = parse_overrides(&toml);

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
        overrides,
    })
}

/// Resolve a dynamic version based on the build backend.
fn resolve_dynamic_version(toml: &toml::Table) -> String {
    let backend = toml
        .get("build-system")
        .and_then(|v| v.get("build-backend"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if backend.contains("setuptools_scm") {
        "${{ environ.get('SETUPTOOLS_SCM_PRETEND_VERSION', '0.0.0') }}".to_string()
    } else if backend.contains("hatch") {
        "${{ environ.get('HATCH_BUILD_VERSION', '0.0.0') }}".to_string()
    } else {
        "0.0.0".to_string()
    }
}

/// Create a `serialize::Recipe` from a parsed `pyproject.toml`.
async fn create_recipe_from_pyproject(
    metadata: &PyprojectMetadata,
    use_mapping: bool,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();
    let ov = &metadata.overrides;

    // -- context --------------------------------------------------------
    recipe
        .context
        .insert("version".to_string(), metadata.version.clone());
    recipe
        .context
        .insert("build_number".to_string(), "0".to_string());
    for (k, v) in &ov.context {
        recipe.context.insert(k.clone(), v.clone());
    }

    // -- package --------------------------------------------------------
    recipe.package.name = ov
        .package_name
        .clone()
        .unwrap_or_else(|| metadata.name.to_lowercase());
    recipe.package.version = "${{ version }}".to_string();

    // -- build ----------------------------------------------------------
    recipe.build.number = "${{ build_number }}".to_string();
    recipe.build.noarch = Some(
        ov.noarch
            .clone()
            .unwrap_or_else(|| "python".to_string()),
    );
    recipe.build.script = ov
        .script
        .clone()
        .unwrap_or_else(|| "${{ PYTHON }} -m pip install .".to_string());

    // Entry points from [project.scripts]
    if !metadata.scripts.is_empty() {
        recipe.build.python.entry_points = metadata.scripts.clone();
    }

    // -- source ---------------------------------------------------------
    if let Some(git) = &ov.source_git {
        recipe.source.push(
            GitSourceElement {
                git: git.clone(),
                tag: ov.source_tag.clone(),
                branch: ov.source_branch.clone(),
            }
            .into(),
        );
    } else if let Some(url) = &ov.source_url {
        recipe.source.push(
            UrlSourceElement {
                url: vec![url.clone()],
                sha256: ov.source_sha256.clone(),
                md5: None,
            }
            .into(),
        );
    } else {
        // Placeholder — the user needs to fill this in.
        recipe.source.push(
            UrlSourceElement {
                url: vec!["# TODO: set the source url".to_string()],
                sha256: Some("# TODO: set the sha256 hash".to_string()),
                md5: None,
            }
            .into(),
        );
    }

    // -- requirements ---------------------------------------------------
    let mapping = if use_mapping {
        conda_pypi_name_mapping().await?
    } else {
        &HashMap::new()
    };

    // Host
    if let Some(host) = &ov.host_reqs {
        recipe.requirements.host = host.clone();
    } else {
        if let Some(python_req) = &metadata.requires_python {
            recipe
                .requirements
                .host
                .push(format!("python {}", python_req));
        } else {
            recipe.requirements.host.push("python".to_string());
        }
        for req in &metadata.build_requires {
            let mapped = map_requirement(req, mapping, use_mapping).await;
            recipe.requirements.host.push(mapped);
        }
        recipe.requirements.host.push("pip".to_string());
    }

    // Build (e.g. compilers — empty by default for noarch)
    if let Some(build) = &ov.build_reqs {
        recipe.requirements.build = build.clone();
    }

    // Run
    if let Some(run) = &ov.run_reqs {
        recipe.requirements.run = run.clone();
    } else {
        if let Some(python_req) = &metadata.requires_python {
            recipe
                .requirements
                .run
                .push(format!("python {}", python_req));
        }
        for req in &metadata.dependencies {
            let mapped = map_requirement(req, mapping, use_mapping).await;
            let formatted = format_requirement(&mapped);
            recipe
                .requirements
                .run
                .push(formatted.trim_start_matches("- ").to_string());
        }
    }

    // -- tests ----------------------------------------------------------
    let imports = ov
        .test_imports
        .clone()
        .unwrap_or_else(|| vec![metadata.name.replace('-', "_")]);
    recipe.tests.push(Test::Python(PythonTest {
        python: PythonTestInner {
            imports,
            pip_check: true,
        },
    }));
    if let Some(commands) = &ov.test_commands {
        recipe.tests.push(Test::Script(ScriptTest {
            script: commands.clone(),
        }));
    }

    // -- about ----------------------------------------------------------
    recipe.about.summary = ov
        .summary
        .clone()
        .or_else(|| metadata.description.clone());
    recipe.about.description = ov.description.clone();

    recipe.about.license = ov.license.clone().or_else(|| {
        metadata
            .license_expression
            .clone()
            .or_else(|| metadata.license.clone())
    });

    if let Some(files) = &ov.license_file {
        recipe.about.license_file = files.clone();
    }

    recipe.about.homepage = ov.homepage.clone().or_else(|| {
        metadata
            .urls
            .get("Homepage")
            .or_else(|| metadata.urls.get("homepage"))
            .cloned()
    });
    recipe.about.repository = ov.repository.clone().or_else(|| {
        metadata
            .urls
            .get("Source Code")
            .or_else(|| {
                metadata
                    .urls
                    .get("Source")
                    .or_else(|| metadata.urls.get("source"))
                    .or_else(|| metadata.urls.get("Repository"))
                    .or_else(|| metadata.urls.get("repository"))
            })
            .cloned()
    });
    recipe.about.documentation = ov.documentation.clone().or_else(|| {
        metadata
            .urls
            .get("Documentation")
            .or_else(|| metadata.urls.get("documentation"))
            .or_else(|| metadata.urls.get("Docs"))
            .cloned()
    });

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

/// Generate a recipe from a local `pyproject.toml` and either write it to disk
/// or print it.
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
        assert_eq!(meta.build_requires, vec!["setuptools>=64", "wheel"]);
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

    #[test]
    fn test_dynamic_version_setuptools_scm() {
        let toml = r#"
[build-system]
requires = ["setuptools_scm"]
build-backend = "setuptools_scm.build_meta"

[project]
name = "my-pkg"
dynamic = ["version"]
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert!(meta.version.contains("SETUPTOOLS_SCM_PRETEND_VERSION"));
    }

    #[test]
    fn test_dynamic_version_hatchling() {
        let toml = r#"
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "my-pkg"
dynamic = ["version"]
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert!(meta.version.contains("HATCH_BUILD_VERSION"));
    }

    #[test]
    fn test_dynamic_version_unknown_backend() {
        let toml = r#"
[build-system]
requires = ["flit_core"]
build-backend = "flit_core.buildapi"

[project]
name = "my-pkg"
dynamic = ["version"]
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(meta.version, "0.0.0");
    }

    #[test]
    fn test_overrides_source_url() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.source]
url = "https://example.com/pkg-${{ version }}.tar.gz"
sha256 = "abc123"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.overrides.source_url.as_deref(),
            Some("https://example.com/pkg-${{ version }}.tar.gz")
        );
        assert_eq!(meta.overrides.source_sha256.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_overrides_source_git() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.source]
git = "https://github.com/org/pkg.git"
tag = "v${{ version }}"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.overrides.source_git.as_deref(),
            Some("https://github.com/org/pkg.git")
        );
        assert_eq!(
            meta.overrides.source_tag.as_deref(),
            Some("v${{ version }}")
        );
    }

    #[test]
    fn test_overrides_requirements() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.requirements]
build = ["{{ compiler('c') }}"]
run = ["python", "numpy"]
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.overrides.build_reqs,
            Some(vec!["{{ compiler('c') }}".to_string()])
        );
        assert_eq!(
            meta.overrides.run_reqs,
            Some(vec!["python".to_string(), "numpy".to_string()])
        );
    }

    #[test]
    fn test_overrides_tests() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.tests]
imports = ["pkg", "pkg.submodule"]
commands = ["pkg --help"]
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.overrides.test_imports,
            Some(vec!["pkg".to_string(), "pkg.submodule".to_string()])
        );
        assert_eq!(
            meta.overrides.test_commands,
            Some(vec!["pkg --help".to_string()])
        );
    }

    #[test]
    fn test_overrides_about() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.about]
license = "MIT"
license_file = ["LICENSE", "NOTICE"]
homepage = "https://override.example.com"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(meta.overrides.license.as_deref(), Some("MIT"));
        assert_eq!(
            meta.overrides.license_file,
            Some(vec!["LICENSE".to_string(), "NOTICE".to_string()])
        );
        assert_eq!(
            meta.overrides.homepage.as_deref(),
            Some("https://override.example.com")
        );
    }

    #[test]
    fn test_overrides_context() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.context]
custom_var = "hello"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.overrides.context.get("custom_var").map(|s| s.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn test_overrides_build() {
        let toml = r#"
[project]
name = "pkg"
version = "1.0"

[tool.rattler-build.build]
script = "python setup.py install"
noarch = "generic"
"#;
        let meta = parse_pyproject(toml).unwrap();
        assert_eq!(
            meta.overrides.script.as_deref(),
            Some("python setup.py install")
        );
        assert_eq!(meta.overrides.noarch.as_deref(), Some("generic"));
    }

    #[tokio::test]
    async fn test_create_recipe_no_overrides() {
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
            urls: HashMap::from([("Homepage".into(), "https://example.com".into())]),
            overrides: RecipeOverrides::default(),
        };

        let recipe = create_recipe_from_pyproject(&meta, false).await.unwrap();
        assert_eq!(recipe.package.name, "my-package");
        assert_eq!(recipe.package.version, "${{ version }}");
        assert_eq!(recipe.about.license, Some("MIT".into()));
        assert_eq!(recipe.about.homepage, Some("https://example.com".into()));
        assert_eq!(
            recipe.build.python.entry_points,
            vec!["my-cli = my_package.cli:main"]
        );
        assert!(recipe
            .requirements
            .host
            .contains(&"python >=3.9".to_string()));
        assert!(recipe
            .requirements
            .run
            .contains(&"python >=3.9".to_string()));
        // source should be a TODO placeholder
        assert!(matches!(&recipe.source[0], serialize::SourceElement::Url(u) if u.url[0].contains("TODO")));
    }

    #[tokio::test]
    async fn test_create_recipe_with_source_url_override() {
        let meta = PyprojectMetadata {
            name: "pkg".into(),
            version: "2.0.0".into(),
            description: None,
            license: None,
            license_expression: None,
            requires_python: None,
            dependencies: vec![],
            build_requires: vec![],
            scripts: vec![],
            urls: HashMap::new(),
            overrides: RecipeOverrides {
                source_url: Some("https://example.com/pkg-${{ version }}.tar.gz".into()),
                source_sha256: Some("deadbeef".into()),
                ..Default::default()
            },
        };

        let recipe = create_recipe_from_pyproject(&meta, false).await.unwrap();
        match &recipe.source[0] {
            serialize::SourceElement::Url(u) => {
                assert_eq!(u.url[0], "https://example.com/pkg-${{ version }}.tar.gz");
                assert_eq!(u.sha256.as_deref(), Some("deadbeef"));
            }
            _ => panic!("expected URL source"),
        }
    }

    #[tokio::test]
    async fn test_create_recipe_with_git_override() {
        let meta = PyprojectMetadata {
            name: "pkg".into(),
            version: "2.0.0".into(),
            description: None,
            license: None,
            license_expression: None,
            requires_python: None,
            dependencies: vec![],
            build_requires: vec![],
            scripts: vec![],
            urls: HashMap::new(),
            overrides: RecipeOverrides {
                source_git: Some("https://github.com/org/pkg.git".into()),
                source_tag: Some("v${{ version }}".into()),
                ..Default::default()
            },
        };

        let recipe = create_recipe_from_pyproject(&meta, false).await.unwrap();
        match &recipe.source[0] {
            serialize::SourceElement::Git(g) => {
                assert_eq!(g.git, "https://github.com/org/pkg.git");
                assert_eq!(g.tag.as_deref(), Some("v${{ version }}"));
            }
            _ => panic!("expected Git source"),
        }
    }

    #[tokio::test]
    async fn test_create_recipe_with_requirements_override() {
        let meta = PyprojectMetadata {
            name: "pkg".into(),
            version: "1.0.0".into(),
            description: None,
            license: None,
            license_expression: None,
            requires_python: None,
            dependencies: vec!["should-be-ignored".into()],
            build_requires: vec![],
            scripts: vec![],
            urls: HashMap::new(),
            overrides: RecipeOverrides {
                run_reqs: Some(vec!["python".into(), "custom-dep".into()]),
                build_reqs: Some(vec!["{{ compiler('c') }}".into()]),
                ..Default::default()
            },
        };

        let recipe = create_recipe_from_pyproject(&meta, false).await.unwrap();
        // The override completely replaces the auto-generated run deps
        assert_eq!(recipe.requirements.run, vec!["python", "custom-dep"]);
        assert_eq!(recipe.requirements.build, vec!["{{ compiler('c') }}"]);
    }

    #[tokio::test]
    async fn test_create_recipe_with_test_overrides() {
        let meta = PyprojectMetadata {
            name: "pkg".into(),
            version: "1.0.0".into(),
            description: None,
            license: None,
            license_expression: None,
            requires_python: None,
            dependencies: vec![],
            build_requires: vec![],
            scripts: vec![],
            urls: HashMap::new(),
            overrides: RecipeOverrides {
                test_imports: Some(vec!["pkg".into(), "pkg.core".into()]),
                test_commands: Some(vec!["pkg --version".into()]),
                ..Default::default()
            },
        };

        let recipe = create_recipe_from_pyproject(&meta, false).await.unwrap();
        assert_eq!(recipe.tests.len(), 2);
        match &recipe.tests[0] {
            Test::Python(p) => {
                assert_eq!(p.python.imports, vec!["pkg", "pkg.core"]);
            }
            _ => panic!("expected python test"),
        }
        match &recipe.tests[1] {
            Test::Script(s) => {
                assert_eq!(s.script, vec!["pkg --version"]);
            }
            _ => panic!("expected script test"),
        }
    }
}
