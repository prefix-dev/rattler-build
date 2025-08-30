use clap::Parser;
use fs_err as fs;
use indexmap::IndexMap;
use miette::{IntoDiagnostic, WrapErr};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::recipe_generator::serialize;

#[derive(Debug, Clone, Parser)]
pub struct PyprojectOpts {
    /// Path to the pyproject.toml file (defaults to pyproject.toml in current directory)
    #[arg(short, long, default_value = "pyproject.toml")]
    pub input: PathBuf,

    /// Path to write the recipe.yaml file. If not provided, output will be printed to stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Whether to overwrite existing recipe file
    #[arg(long)]
    pub overwrite: bool,

    /// Output format: yaml or json
    #[arg(long, default_value = "yaml")]
    pub format: String,

    /// Sort keys in output
    #[arg(long)]
    pub sort_keys: bool,

    /// Include helpful comments in the output
    #[arg(long, default_value = "true")]
    pub include_comments: bool,

    /// Exclude specific sections from the output (comma-separated)
    #[arg(long)]
    pub exclude_sections: Option<String>,

    /// Validate the generated recipe
    #[arg(long, default_value = "true")]
    pub validate: bool,
}

/// Generate a recipe from a pyproject.toml file
pub async fn generate_pyproject_recipe(opts: &PyprojectOpts) -> miette::Result<()> {
    tracing::info!("Generating recipe from {}", opts.input.display());

    // Check if input file exists
    if !opts.input.exists() {
        return Err(miette::miette!(
            "pyproject.toml file not found: {}",
            opts.input.display()
        ));
    }

    // Load and parse pyproject.toml
    let toml_data = load_pyproject_toml(&opts.input)?;

    // Generate the recipe
    let project_root = opts
        .input
        .parent()
        .unwrap_or(&PathBuf::from("."))
        .to_path_buf();
    let recipe = assemble_recipe(toml_data, &project_root).wrap_err("Failed to assemble recipe")?;

    // Convert to the requested format
    let recipe_content = match opts.format.as_str() {
        "json" => {
            let json_value = serde_json::to_value(&recipe).into_diagnostic()?;
            serde_json::to_string_pretty(&json_value).into_diagnostic()?
        }
        _ => {
            // Convert to YAML and add schema comment
            let yaml_content = serde_yaml::to_string(&recipe).into_diagnostic()?;
            format_yaml_with_schema(&yaml_content)
        }
    };

    // Write or print the recipe
    if let Some(output_path) = &opts.output {
        // Check if output file exists and we're not overwriting
        if output_path.exists() && !opts.overwrite {
            return Err(miette::miette!(
                "Output file {} already exists. Use --overwrite to replace it.",
                output_path.display()
            ));
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).into_diagnostic()?;
        }

        // Write to the specified output file
        fs::write(output_path, &recipe_content).into_diagnostic()?;
        tracing::info!("Recipe written to {}", output_path.display());
    } else {
        print!("{}", recipe_content);
    }

    Ok(())
}

/// Load and parse a pyproject.toml file
fn load_pyproject_toml(path: &PathBuf) -> miette::Result<HashMap<String, Value>> {
    let content = fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read {}", path.display()))?;

    let toml_value: toml::Value = toml::from_str(&content)
        .into_diagnostic()
        .wrap_err("Failed to parse pyproject.toml")?;

    // Convert to JSON Value for easier manipulation
    let json_str = serde_json::to_string(&toml_value).into_diagnostic()?;
    let json_value: HashMap<String, Value> = serde_json::from_str(&json_str).into_diagnostic()?;

    Ok(json_value)
}

/// Assemble a complete recipe from pyproject.toml data
fn assemble_recipe(
    toml_data: HashMap<String, Value>,
    _project_root: &Path,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();

    // Extract project metadata
    let project = toml_data
        .get("project")
        .and_then(|p| p.as_object())
        .ok_or_else(|| miette::miette!("No [project] section found in pyproject.toml"))?;

    // Build base sections from [project] metadata
    let context = build_context_section(project, &toml_data)?;
    recipe.context = context;

    recipe.package = build_package_section(project)?;
    recipe.source = build_source_section(project, &toml_data)?;
    recipe.build = build_build_section(&toml_data)?;
    recipe.requirements = build_requirements_section(project, &toml_data)?;

    if let Some(test_section) = build_test_section(project, &toml_data)? {
        recipe.tests.push(test_section);
    }

    recipe.about = build_about_section(project)?;

    // Handle schema version from tool.conda.recipe or set default
    recipe.schema_version = build_schema_version(&toml_data);

    // Apply conda-specific overrides from tool.conda.recipe.* sections
    // This mirrors the pyrattler-recipe-autogen approach where each section
    // can be overridden via tool.conda.recipe.<section_name>
    apply_conda_recipe_overrides(&mut recipe, &toml_data)?;

    Ok(recipe)
}

/// Build the context section
fn build_context_section(
    project: &serde_json::Map<String, Value>,
    toml_data: &HashMap<String, Value>,
) -> miette::Result<IndexMap<String, String>> {
    let mut context = IndexMap::new();

    // Extract name and version
    let name = project
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| miette::miette!("Project name not found"))?;

    let version = if let Some(v) = project.get("version").and_then(|v| v.as_str()) {
        v.to_string()
    } else {
        // Check for dynamic version
        let default_dynamic = vec![];
        let dynamic = project
            .get("dynamic")
            .and_then(|d| d.as_array())
            .unwrap_or(&default_dynamic);

        if dynamic.iter().any(|d| d.as_str() == Some("version")) {
            // Try to resolve dynamic version
            resolve_dynamic_version(toml_data)?
        } else {
            return Err(miette::miette!("No version found in project metadata"));
        }
    };

    context.insert("name".to_string(), name.to_lowercase().replace(" ", "-"));
    context.insert("version".to_string(), version);

    // Extract Python version requirement
    if let Some(requires_python) = project.get("requires-python").and_then(|r| r.as_str()) {
        if let Some(min_version) = extract_min_python_version(requires_python) {
            context.insert("python_min".to_string(), min_version);
        }
    }

    Ok(context)
}

/// Build the package section
fn build_package_section(
    _project: &serde_json::Map<String, Value>,
) -> miette::Result<serialize::Package> {
    Ok(serialize::Package {
        name: "${{ name }}".to_string(),
        version: "${{ version }}".to_string(),
    })
}

/// Build the source section
fn build_source_section(
    project: &serde_json::Map<String, Value>,
    _toml_data: &HashMap<String, Value>,
) -> miette::Result<Vec<serialize::SourceElement>> {
    let name = project
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("package");

    // Check for explicit source URLs in project.urls
    if let Some(urls) = project.get("urls").and_then(|u| u.as_object()) {
        if let Some(source_url) = urls
            .get("Source")
            .or_else(|| urls.get("Homepage"))
            .and_then(|u| u.as_str())
        {
            if source_url.contains("github.com") || source_url.contains("gitlab.com") {
                // Git repository source
                return Ok(vec![serialize::SourceElement::Url(
                    serialize::UrlSourceElement {
                        url: vec![format!(
                            "{}/archive/v${{{{ version }}}}.tar.gz",
                            source_url.trim_end_matches('/')
                        )],
                        sha256: None,
                        md5: None,
                    },
                )]);
            }
        }
    }

    // Default to PyPI source
    let package_name = name.to_lowercase().replace("-", "_");
    let pypi_url = format!(
        "https://pypi.org/packages/source/{}/{}/{}-${{{{ version }}}}.tar.gz",
        &package_name[..1],
        package_name,
        package_name
    );

    Ok(vec![serialize::SourceElement::Url(
        serialize::UrlSourceElement {
            url: vec![pypi_url],
            sha256: None,
            md5: None,
        },
    )])
}

/// Build the build section
fn build_build_section(toml_data: &HashMap<String, Value>) -> miette::Result<serialize::Build> {
    let mut build = serialize::Build {
        script: "${{ PYTHON }} -m pip install . -vv --no-build-isolation".to_string(),
        number: Some(0),
        noarch: Some("python".to_string()),
        ..Default::default()
    };

    // Check for entry points
    if let Some(project) = toml_data.get("project").and_then(|p| p.as_object()) {
        if let Some(scripts) = project.get("scripts").and_then(|s| s.as_object()) {
            let mut entry_points = Vec::new();
            for (name, command) in scripts {
                if let Some(cmd) = command.as_str() {
                    entry_points.push(format!("{} = {}", name, cmd));
                }
            }
            if !entry_points.is_empty() {
                build.python.entry_points = entry_points;
            }
        }
    }

    Ok(build)
}

/// Build the requirements section
fn build_requirements_section(
    project: &serde_json::Map<String, Value>,
    toml_data: &HashMap<String, Value>,
) -> miette::Result<serialize::Requirements> {
    let mut requirements = serialize::Requirements {
        build: vec![],
        ..Default::default()
    };

    // Host requirements - Python and pip, plus build system requirements
    let mut host_deps = vec!["python".to_string(), "pip".to_string()];

    // Add Python version constraint if specified in requires-python
    if let Some(requires_python) = project.get("requires-python").and_then(|r| r.as_str()) {
        host_deps[0] = format_python_constraint(requires_python);
    }

    // Add build system requirements
    if let Some(build_system) = toml_data.get("build-system").and_then(|b| b.as_object()) {
        if let Some(requires) = build_system.get("requires").and_then(|r| r.as_array()) {
            for req in requires {
                if let Some(req_str) = req.as_str() {
                    host_deps.push(req_str.to_string());
                }
            }
        }
    }

    requirements.host = host_deps;

    // Runtime requirements - Python plus all project dependencies
    let mut run_deps = vec![];

    // Add Python constraint first
    if let Some(requires_python) = project.get("requires-python").and_then(|r| r.as_str()) {
        run_deps.push(format_python_constraint(requires_python));
    } else {
        run_deps.push("python".to_string());
    }

    // Add project dependencies exactly as specified (following pyrattler-recipe-autogen pattern)
    if let Some(deps) = project.get("dependencies").and_then(|d| d.as_array()) {
        for dep in deps {
            if let Some(dep_str) = dep.as_str() {
                // Convert Python dependency format to conda format
                let conda_dep = convert_python_to_conda_dependency(dep_str);
                run_deps.push(conda_dep);
            }
        }
    }

    requirements.run = run_deps;

    Ok(requirements)
}

/// Build the test section
fn build_test_section(
    project: &serde_json::Map<String, Value>,
    _toml_data: &HashMap<String, Value>,
) -> miette::Result<Option<serialize::Test>> {
    let name = project
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("package");

    // Create a simple import test
    let import_name = name.to_lowercase().replace("-", "_");

    Ok(Some(serialize::Test::Python(serialize::PythonTest {
        python: serialize::PythonTestInner {
            imports: vec![import_name],
            pip_check: true,
        },
    })))
}

/// Build the about section
fn build_about_section(
    project: &serde_json::Map<String, Value>,
) -> miette::Result<serialize::About> {
    let mut about = serialize::About {
        summary: project
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string()),
        license: project
            .get("license")
            .and_then(|l| l.as_object())
            .and_then(|l| l.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        ..Default::default()
    };

    // Extract URLs
    if let Some(urls) = project.get("urls").and_then(|u| u.as_object()) {
        about.homepage = urls
            .get("Homepage")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string());
        about.repository = urls
            .get("Source")
            .or_else(|| urls.get("Repository"))
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());
        about.documentation = urls
            .get("Documentation")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());
    }

    Ok(about)
}

/// Build schema version from tool.conda.recipe.schema_version or use default
fn build_schema_version(toml_data: &HashMap<String, Value>) -> Option<u32> {
    // Check for tool.conda.recipe.schema_version
    if let Some(tool) = toml_data.get("tool").and_then(|t| t.as_object()) {
        if let Some(conda) = tool.get("conda").and_then(|c| c.as_object()) {
            if let Some(recipe) = conda.get("recipe").and_then(|r| r.as_object()) {
                if let Some(schema_version) = recipe.get("schema_version").and_then(|v| v.as_u64())
                {
                    return Some(schema_version as u32);
                }
            }
        }
    }

    // Default schema version if not specified
    Some(1)
}

/// Resolve dynamic version from build system
fn resolve_dynamic_version(toml_data: &HashMap<String, Value>) -> miette::Result<String> {
    // Check build system for version resolution
    if let Some(build_system) = toml_data.get("build-system").and_then(|b| b.as_object()) {
        if let Some(backend) = build_system.get("build-backend").and_then(|b| b.as_str()) {
            if backend.contains("setuptools_scm") {
                return Ok(
                    "${{ environ.get('SETUPTOOLS_SCM_PRETEND_VERSION', '0.1.0') }}".to_string(),
                );
            } else if backend.contains("hatch") {
                return Ok("${{ environ.get('HATCH_BUILD_VERSION', '0.1.0') }}".to_string());
            }
        }
    }

    // Default fallback
    Ok("0.1.0".to_string())
}

/// Extract minimum Python version from requires-python string
fn extract_min_python_version(requires_python: &str) -> Option<String> {
    // Simple regex to extract version like ">=3.8" -> "3.8"
    if let Ok(re) = Regex::new(r">=\s*([0-9]+\.[0-9]+)") {
        if let Some(captures) = re.captures(requires_python) {
            return captures.get(1).map(|m| m.as_str().to_string());
        }
    }
    None
}

/// Format YAML content with schema comment at the top
fn format_yaml_with_schema(yaml_content: &str) -> String {
    let schema_comment = "# yaml-language-server: $schema=https://raw.githubusercontent.com/prefix-dev/recipe-format/main/schema.json";
    format!("{}\n{}", schema_comment, yaml_content)
}

/// Format Python version constraint for conda
fn format_python_constraint(requires_python: &str) -> String {
    // Convert requires-python format to conda format
    // e.g., ">=3.9" -> "python >=3.9"
    // e.g., ">=3.9,<4.0" -> "python >=3.9,<4.0"
    format!("python {}", requires_python)
}

/// Convert Python dependency format to conda dependency format
/// Following the same pattern as pyrattler-recipe-autogen
fn convert_python_to_conda_dependency(dep: &str) -> String {
    // Handle environment markers (e.g., 'package>=1.0; python_version >= "3.8"')
    let base_dep = if dep.contains(';') {
        dep.split(';').next().unwrap_or(dep).trim()
    } else {
        dep
    };

    // Convert Python version operators to conda format
    // Process in order from longest to shortest to avoid conflicts
    let mut conda_dep = base_dep.to_string();

    // Handle multi-character operators first
    conda_dep = conda_dep.replace("==", " ="); // Python == becomes conda =
    conda_dep = conda_dep.replace("~=", " ~="); // Compatible release stays the same
    conda_dep = conda_dep.replace(">=", " >="); // Greater than or equal
    conda_dep = conda_dep.replace("<=", " <="); // Less than or equal
    conda_dep = conda_dep.replace("!=", " !="); // Not equal

    // Handle single character operators, but only if not already processed
    // and not immediately after a comma
    let chars: Vec<char> = conda_dep.chars().collect();
    let mut result = String::new();

    for (i, &ch) in chars.iter().enumerate() {
        if (ch == '>' || ch == '<') && i > 0 {
            let prev_char = chars[i - 1];
            // Add space only if previous char is not space and not comma
            if prev_char != ' ' && prev_char != ',' {
                result.push(' ');
            }
        }
        result.push(ch);
    }

    let conda_dep = result;

    // Handle common Python package to conda package name mappings
    // This is a subset - in a full implementation this would be more comprehensive
    apply_package_name_mapping(&conda_dep)
}

/// Apply common Python package name to conda package name mappings
fn apply_package_name_mapping(dep: &str) -> String {
    // Common package name mappings from PyPI to conda-forge
    let mappings = [
        ("pillow", "pillow"),
        ("pyyaml", "pyyaml"),
        ("scikit-learn", "scikit-learn"),
        ("beautifulsoup4", "beautifulsoup4"),
        ("python-dateutil", "python-dateutil"),
        // Add more mappings as needed
    ];

    let mut result = dep.to_string();

    for (pypi_name, conda_name) in mappings {
        if result.starts_with(pypi_name) {
            result = result.replace(pypi_name, conda_name);
            break;
        }
    }

    result
}

/// Apply conda-specific overrides from tool.conda.recipe.* sections
/// This follows the same pattern as pyrattler-recipe-autogen where each recipe section
/// can be overridden via tool.conda.recipe.<section_name>
fn apply_conda_recipe_overrides(
    recipe: &mut serialize::Recipe,
    toml_data: &HashMap<String, Value>,
) -> miette::Result<()> {
    // Get the tool.conda.recipe section if it exists
    let conda_recipe_config = toml_data
        .get("tool")
        .and_then(|tool| tool.as_object())
        .and_then(|tool| tool.get("conda"))
        .and_then(|conda| conda.as_object())
        .and_then(|conda| conda.get("recipe"))
        .and_then(|recipe| recipe.as_object());

    let conda_recipe_config = match conda_recipe_config {
        Some(config) => config,
        None => return Ok(()), // No conda recipe config found
    };

    // Apply overrides following pyrattler-recipe-autogen pattern:

    // 1. tool.conda.recipe.context - override context variables
    if let Some(context_override) = conda_recipe_config
        .get("context")
        .and_then(|c| c.as_object())
    {
        apply_context_overrides(&mut recipe.context, context_override)?;
    }

    // 2. tool.conda.recipe.package - override package metadata
    if let Some(package_override) = conda_recipe_config
        .get("package")
        .and_then(|p| p.as_object())
    {
        apply_package_overrides(&mut recipe.package, package_override)?;
    }

    // 3. tool.conda.recipe.source - override source section
    if let Some(source_override) = conda_recipe_config
        .get("source")
        .and_then(|s| s.as_object())
    {
        apply_source_overrides(&mut recipe.source, source_override)?;
    }

    // 4. tool.conda.recipe.build - override build section
    if let Some(build_override) = conda_recipe_config.get("build").and_then(|b| b.as_object()) {
        apply_build_overrides(&mut recipe.build, build_override)?;
    }

    // 5. tool.conda.recipe.requirements - override requirements section
    if let Some(req_override) = conda_recipe_config
        .get("requirements")
        .and_then(|r| r.as_object())
    {
        apply_requirements_overrides(&mut recipe.requirements, req_override)?;
    }

    // 6. tool.conda.recipe.test - override test section
    if let Some(test_override) = conda_recipe_config.get("test").and_then(|t| t.as_object()) {
        apply_test_overrides(&mut recipe.tests, test_override)?;
    }

    // 7. tool.conda.recipe.about - override about section
    if let Some(about_override) = conda_recipe_config.get("about").and_then(|a| a.as_object()) {
        apply_about_overrides(&mut recipe.about, about_override)?;
    }

    Ok(())
}

/// Apply context section overrides from tool.conda.recipe.context
fn apply_context_overrides(
    context: &mut IndexMap<String, String>,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    for (key, value) in config {
        if let Some(string_value) = value.as_str() {
            context.insert(key.clone(), string_value.to_string());
        }
    }
    Ok(())
}

/// Apply package section overrides from tool.conda.recipe.package  
fn apply_package_overrides(
    package: &mut serialize::Package,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    if let Some(name) = config.get("name").and_then(|n| n.as_str()) {
        package.name = name.to_string();
    }

    if let Some(version) = config.get("version").and_then(|v| v.as_str()) {
        package.version = version.to_string();
    }

    Ok(())
}

/// Apply source section overrides from tool.conda.recipe.source
fn apply_source_overrides(
    sources: &mut Vec<serialize::SourceElement>,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    // If config contains a complete source definition, replace existing sources
    if config.contains_key("url") || config.contains_key("git") || config.contains_key("path") {
        sources.clear();

        if let Some(url) = config.get("url").and_then(|u| u.as_str()) {
            let mut url_source = serialize::UrlSourceElement {
                url: vec![url.to_string()],
                ..Default::default()
            };

            // Add optional fields
            if let Some(sha256) = config.get("sha256").and_then(|s| s.as_str()) {
                url_source.sha256 = Some(sha256.to_string());
            }
            if let Some(md5) = config.get("md5").and_then(|m| m.as_str()) {
                url_source.md5 = Some(md5.to_string());
            }

            sources.push(serialize::SourceElement::Url(url_source));
        } else if let Some(git_url) = config.get("git").and_then(|g| g.as_str()) {
            let mut git_source = serialize::GitSourceElement {
                git: git_url.to_string(),
                ..Default::default()
            };

            if let Some(tag) = config.get("tag").and_then(|t| t.as_str()) {
                git_source.tag = Some(tag.to_string());
            }
            if let Some(branch) = config.get("branch").and_then(|b| b.as_str()) {
                git_source.branch = Some(branch.to_string());
            }

            sources.push(serialize::SourceElement::Git(git_source));
        }
        // Note: Path sources would be handled here if the serialize module supported them
    } else {
        // Partial updates to existing source
        if !sources.is_empty() {
            if let serialize::SourceElement::Url(url_source) = &mut sources[0] {
                if let Some(sha256) = config.get("sha256").and_then(|s| s.as_str()) {
                    url_source.sha256 = Some(sha256.to_string());
                }
                if let Some(md5) = config.get("md5").and_then(|m| m.as_str()) {
                    url_source.md5 = Some(md5.to_string());
                }
            }
        }
    }

    Ok(())
}

/// Apply build section overrides from tool.conda.recipe.build
fn apply_build_overrides(
    build: &mut serialize::Build,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    if let Some(script) = config.get("script").and_then(|s| s.as_str()) {
        build.script = script.to_string();
    }

    if let Some(noarch) = config.get("noarch").and_then(|n| n.as_str()) {
        build.noarch = Some(noarch.to_string());
    }

    if let Some(number) = config.get("number").and_then(|n| n.as_u64()) {
        build.number = Some(number as u32);
    }

    // Handle python section overrides
    if let Some(python_config) = config.get("python").and_then(|p| p.as_object()) {
        if let Some(entry_points) = python_config
            .get("entry_points")
            .and_then(|ep| ep.as_array())
        {
            build.python.entry_points = entry_points
                .iter()
                .filter_map(|ep| ep.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    // Handle skip conditions if present
    if let Some(skip) = config.get("skip").and_then(|s| s.as_array()) {
        // Note: The serialize module doesn't currently have a skip field,
        // but this shows where it would be handled
        tracing::info!(
            "Skip conditions found but not yet supported in serialize module: {:?}",
            skip
        );
    }

    Ok(())
}

/// Apply requirements section overrides from tool.conda.recipe.requirements
fn apply_requirements_overrides(
    requirements: &mut serialize::Requirements,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    // Handle build requirements
    if let Some(build) = config.get("build").and_then(|b| b.as_array()) {
        requirements.build = build
            .iter()
            .filter_map(|dep| dep.as_str().map(|s| s.to_string()))
            .collect();
    }

    // Handle host requirements
    if let Some(host) = config.get("host").and_then(|h| h.as_array()) {
        requirements.host = host
            .iter()
            .filter_map(|dep| dep.as_str().map(|s| s.to_string()))
            .collect();
    }

    // Handle run requirements
    if let Some(run) = config.get("run").and_then(|r| r.as_array()) {
        requirements.run = run
            .iter()
            .filter_map(|dep| dep.as_str().map(|s| s.to_string()))
            .collect();
    }

    // Note: pyrattler-recipe-autogen also supports conditional requirements
    // with selectors like:
    // run_constrained, run_exports, etc. These could be added here
    // when the serialize module supports them

    Ok(())
}

/// Apply test section overrides from tool.conda.recipe.test
fn apply_test_overrides(
    tests: &mut Vec<serialize::Test>,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    // If we have test configuration, ensure we have at least one test
    if tests.is_empty() {
        tests.push(serialize::Test::Python(serialize::PythonTest::default()));
    }

    // Handle python test configuration
    if let Some(python_config) = config.get("python").and_then(|p| p.as_object()) {
        // Ensure we have a Python test
        if let serialize::Test::Python(python_test) = &mut tests[0] {
            if let Some(imports) = python_config.get("imports").and_then(|i| i.as_array()) {
                python_test.python.imports = imports
                    .iter()
                    .filter_map(|imp| imp.as_str().map(|s| s.to_string()))
                    .collect();
            }

            if let Some(pip_check) = python_config.get("pip_check").and_then(|pc| pc.as_bool()) {
                python_test.python.pip_check = pip_check;
            }
        }
    }

    // Handle script-based test commands
    if let Some(commands) = config.get("commands").and_then(|c| c.as_array()) {
        let script_commands: Vec<String> = commands
            .iter()
            .filter_map(|cmd| cmd.as_str().map(|s| s.to_string()))
            .collect();

        if !script_commands.is_empty() {
            let script_test = serialize::Test::Script(serialize::ScriptTest {
                script: script_commands,
            });
            tests.push(script_test);
        }
    }

    // Handle test requirements (if supported in the future)
    if let Some(requires) = config.get("requires").and_then(|r| r.as_array()) {
        let _test_requires: Vec<String> = requires
            .iter()
            .filter_map(|req| req.as_str().map(|s| s.to_string()))
            .collect();
        // Note: Test requirements would be added here when serialize module supports them
        tracing::info!("Test requirements found but not yet supported in serialize module");
    }

    Ok(())
}

/// Apply about section overrides from tool.conda.recipe.about
fn apply_about_overrides(
    about: &mut serialize::About,
    config: &serde_json::Map<String, Value>,
) -> miette::Result<()> {
    if let Some(homepage) = config.get("homepage").and_then(|h| h.as_str()) {
        about.homepage = Some(homepage.to_string());
    }

    if let Some(summary) = config.get("summary").and_then(|s| s.as_str()) {
        about.summary = Some(summary.to_string());
    }

    if let Some(description) = config.get("description").and_then(|d| d.as_str()) {
        about.description = Some(description.to_string());
    }

    if let Some(license) = config.get("license").and_then(|l| l.as_str()) {
        about.license = Some(license.to_string());
    }

    if let Some(license_file) = config.get("license_file") {
        match license_file {
            Value::String(file) => {
                about.license_file = Some(file.clone());
            }
            Value::Array(files) => {
                let file_strings: Vec<String> = files
                    .iter()
                    .filter_map(|f| f.as_str().map(|s| s.to_string()))
                    .collect();
                if !file_strings.is_empty() {
                    // For now, take the first file. In future, serialize module might support arrays
                    about.license_file = Some(file_strings[0].clone());
                }
            }
            _ => {}
        }
    }

    if let Some(repository) = config.get("repository").and_then(|r| r.as_str()) {
        about.repository = Some(repository.to_string());
    }

    if let Some(documentation) = config.get("documentation").and_then(|d| d.as_str()) {
        about.documentation = Some(documentation.to_string());
    }

    // Handle common aliases used in pyrattler-recipe-autogen
    if let Some(doc_url) = config.get("doc_url").and_then(|d| d.as_str()) {
        about.documentation = Some(doc_url.to_string());
    }

    if let Some(dev_url) = config.get("dev_url").and_then(|d| d.as_str()) {
        about.repository = Some(dev_url.to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_convert_python_to_conda_dependency() {
        // Test basic version constraints
        assert_eq!(
            convert_python_to_conda_dependency("numpy>=1.21.0"),
            "numpy >=1.21.0"
        );
        assert_eq!(
            convert_python_to_conda_dependency("pandas==1.3.0"),
            "pandas =1.3.0"
        );
        assert_eq!(
            convert_python_to_conda_dependency("requests~=2.25.0"),
            "requests ~=2.25.0"
        );

        // Test environment markers
        assert_eq!(
            convert_python_to_conda_dependency("typing-extensions>=3.7; python_version<'3.8'"),
            "typing-extensions >=3.7"
        );

        // Test multiple constraints
        assert_eq!(
            convert_python_to_conda_dependency("click>=7.0,<9.0"),
            "click >=7.0,<9.0"
        );
    }

    #[test]
    fn test_format_python_constraint() {
        assert_eq!(format_python_constraint(">=3.9"), "python >=3.9");
        assert_eq!(format_python_constraint(">=3.9,<4.0"), "python >=3.9,<4.0");
    }

    #[test]
    fn test_apply_package_name_mapping() {
        assert_eq!(
            apply_package_name_mapping("pillow >=8.0.0"),
            "pillow >=8.0.0"
        );
        assert_eq!(
            apply_package_name_mapping("pyyaml >=5.4.0"),
            "pyyaml >=5.4.0"
        );
    }

    #[test]
    fn test_build_schema_version() {
        // Test default schema version
        let toml_data = HashMap::new();
        assert_eq!(build_schema_version(&toml_data), Some(1));

        // Test custom schema version
        let mut toml_data = HashMap::new();
        let tool_data = json!({
            "conda": {
                "recipe": {
                    "schema_version": 2
                }
            }
        });
        toml_data.insert("tool".to_string(), tool_data);
        assert_eq!(build_schema_version(&toml_data), Some(2));

        // Test missing schema version in tool.conda.recipe
        let mut toml_data = HashMap::new();
        let tool_data = json!({
            "conda": {
                "recipe": {}
            }
        });
        toml_data.insert("tool".to_string(), tool_data);
        assert_eq!(build_schema_version(&toml_data), Some(1));
    }

    #[test]
    fn test_format_yaml_with_schema() {
        let yaml_content = "schema_version: 1\npackage:\n  name: test";
        let result = format_yaml_with_schema(yaml_content);
        assert!(result.starts_with("# yaml-language-server: $schema=https://raw.githubusercontent.com/prefix-dev/recipe-format/main/schema.json"));
        assert!(result.contains("schema_version: 1"));
    }

    #[test]
    fn test_build_context_section() {
        let mut project = serde_json::Map::new();
        project.insert("name".to_string(), json!("test-package"));
        project.insert("version".to_string(), json!("1.0.0"));

        let toml_data = HashMap::new();
        let context = build_context_section(&project, &toml_data).unwrap();

        assert_eq!(context.get("name"), Some(&"test-package".to_string()));
        assert_eq!(context.get("version"), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_build_context_section_dynamic_version() {
        let mut project = serde_json::Map::new();
        project.insert("name".to_string(), json!("test-package"));
        project.insert("dynamic".to_string(), json!(["version"]));

        let mut toml_data = HashMap::new();
        let build_system = json!({
            "build-backend": "hatchling.build"
        });
        toml_data.insert("build-system".to_string(), build_system);

        let context = build_context_section(&project, &toml_data).unwrap();
        assert_eq!(context.get("name"), Some(&"test-package".to_string()));
        assert_eq!(
            context.get("version"),
            Some(&"${{ environ.get('HATCH_BUILD_VERSION', '0.1.0') }}".to_string())
        );
    }

    #[test]
    fn test_build_package_section() {
        let mut project = serde_json::Map::new();
        project.insert("name".to_string(), json!("test-package"));
        project.insert("version".to_string(), json!("1.0.0"));

        let package = build_package_section(&project).unwrap();
        assert_eq!(package.name, "${{ name }}");
        assert_eq!(package.version, "${{ version }}");
    }

    #[test]
    fn test_build_requirements_section() {
        let mut project = serde_json::Map::new();
        project.insert("name".to_string(), json!("test-package"));
        project.insert(
            "dependencies".to_string(),
            json!(["numpy>=1.21.0", "pandas>=1.3.0", "click>=8.0.0"]),
        );
        project.insert("requires-python".to_string(), json!(">=3.9"));

        let mut toml_data = HashMap::new();
        let build_system = json!({
            "requires": ["setuptools", "wheel"]
        });
        toml_data.insert("build-system".to_string(), build_system);

        let requirements = build_requirements_section(&project, &toml_data).unwrap();

        // Check host dependencies include build system requirements
        assert!(requirements.host.contains(&"setuptools".to_string()));
        assert!(requirements.host.contains(&"wheel".to_string()));

        // Check run dependencies include project dependencies
        assert!(requirements.run.contains(&"numpy >=1.21.0".to_string()));
        assert!(requirements.run.contains(&"pandas >=1.3.0".to_string()));
        assert!(requirements.run.contains(&"click >=8.0.0".to_string()));

        // Check python constraint is included
        assert!(requirements.run.contains(&"python >=3.9".to_string()));
    }

    #[test]
    fn test_build_about_section() {
        let mut project = serde_json::Map::new();
        project.insert("description".to_string(), json!("A test package"));
        project.insert("license".to_string(), json!({"text": "MIT"}));

        let urls = json!({
            "Homepage": "https://example.com",
            "Source": "https://github.com/example/test",
            "Documentation": "https://docs.example.com"
        });
        project.insert("urls".to_string(), urls);

        let about = build_about_section(&project).unwrap();
        assert_eq!(about.summary, Some("A test package".to_string()));
        assert_eq!(about.license, Some("MIT".to_string()));
        assert_eq!(about.homepage, Some("https://example.com".to_string()));
        assert_eq!(
            about.repository,
            Some("https://github.com/example/test".to_string())
        );
        assert_eq!(
            about.documentation,
            Some("https://docs.example.com".to_string())
        );
    }

    #[test]
    fn test_resolve_dynamic_version() {
        // Test setuptools_scm
        let mut toml_data = HashMap::new();
        let build_system = json!({
            "build-backend": "setuptools_scm.build_meta"
        });
        toml_data.insert("build-system".to_string(), build_system);

        let version = resolve_dynamic_version(&toml_data).unwrap();
        assert_eq!(
            version,
            "${{ environ.get('SETUPTOOLS_SCM_PRETEND_VERSION', '0.1.0') }}"
        );

        // Test hatchling
        let mut toml_data = HashMap::new();
        let build_system = json!({
            "build-backend": "hatchling.build"
        });
        toml_data.insert("build-system".to_string(), build_system);

        let version = resolve_dynamic_version(&toml_data).unwrap();
        assert_eq!(
            version,
            "${{ environ.get('HATCH_BUILD_VERSION', '0.1.0') }}"
        );
    }
}
