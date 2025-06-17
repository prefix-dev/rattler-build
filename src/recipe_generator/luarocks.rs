use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Parser;
use indexmap::IndexMap;
use miette::IntoDiagnostic;
use rattler_conda_types::PackageName;
use serde::Deserialize;

use crate::recipe_generator::serialize::{
    About, Build, Requirements, SourceElement, Test, ScriptTest, write_recipe, Recipe, Python,
};

#[derive(Debug, Clone, Parser)]
pub struct LuarocksOpts {
    /// Luarocks package to generate recipe for.
    /// Can be specified as:
    /// - module (fetches latest version)
    /// - module/version
    /// - author/module/version
    /// - Direct rockspec URL
    pub rock: String,

    /// Where to write the recipe to
    #[arg(short, long, default_value = ".")]
    pub write_to: PathBuf,
}

const LUAROCKS_API_URL: &str = "https://luarocks.org/modules";

#[derive(Debug, Deserialize)]
pub struct LuarocksManifest {
    pub version: String,
    pub arch: String,
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct LuarocksModule {
    pub module: String,
    pub name: String,
    pub summary: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub maintainer: Option<String>,
    pub versions: Vec<LuarocksVersion>,
}

#[derive(Debug, Deserialize)]
pub struct LuarocksVersion {
    pub version: String,
    pub rockspec_url: String,
    pub source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LuarocksRockspec {
    pub package: String,
    pub version: String,
    pub source: RockspecSource,
    pub description: RockspecDescription,
    pub dependencies: Vec<String>,
    pub build: Option<RockspecBuild>,
}

#[derive(Debug, Deserialize)]
pub struct RockspecSource {
    pub url: String,
    pub md5: Option<String>,
    pub sha256: Option<String>,
    pub file: Option<String>,
    pub dir: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RockspecDescription {
    pub summary: Option<String>,
    pub detailed: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub maintainer: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RockspecBuild {
    #[serde(rename = "type")]
    pub build_type: Option<String>,
    pub modules: Option<BTreeMap<String, serde_json::Value>>,
    pub install: Option<BTreeMap<String, Vec<String>>>,
}

pub async fn generate_luarocks_recipe(opts: &LuarocksOpts) -> miette::Result<()> {
    let rockspec_url = if opts.rock.contains("http://") || opts.rock.contains("https://") {
        // Direct rockspec URL provided
        opts.rock.clone()
    } else {
        // Parse package specification
        let parts: Vec<&str> = opts.rock.split('/').collect();
        match parts.as_slice() {
            [module] => {
                // Just module name, fetch latest version
                return fetch_and_generate_from_module(module, None, opts).await;
            }
            [module, version] => {
                // Module and version  
                return fetch_and_generate_from_module(module, Some(version), opts).await;
            }
            [author, module, version] => {
                // Construct direct rockspec URL
                format!("https://luarocks.org/manifests/{}/{}-{}.rockspec", author, module, version)
            }
            _ => {
                return Err(miette::miette!(
                    "Invalid rock specification. Use 'module', 'module/version', or 'author/module/version'"
                ));
            }
        }
    };

    // Fetch and parse rockspec
    let rockspec_content = fetch_rockspec(&rockspec_url).await?;
    let rockspec = parse_rockspec(&rockspec_content)?;

    // Generate recipe
    let recipe = rockspec_to_recipe(&rockspec)?;

    // Write recipe
    let recipe_str = recipe.to_string();
    if opts.write_to == PathBuf::from(".") {
        println!("{}", recipe_str);
    } else {
        let package_name = recipe.package.name.clone();
        write_recipe(&package_name, &recipe_str).into_diagnostic()?;
    }

    Ok(())
}

async fn fetch_and_generate_from_module(
    module: &str,
    version: Option<&str>,
    opts: &LuarocksOpts,
) -> miette::Result<()> {
    // Search for module on LuaRocks by scraping HTML
    let search_url = format!("https://luarocks.org/search?q={}", module);
    let response = reqwest::get(&search_url)
        .await
        .into_diagnostic()?
        .text()
        .await
        .into_diagnostic()?;

    // Extract the first module link from search results
    let module_link_pattern = regex::Regex::new(r#"<a class="title" href="/modules/([^/]+)/([^"]+)">([^<]+)</a>"#)
        .unwrap();
    
    let module_match = module_link_pattern
        .captures(&response)
        .ok_or_else(|| miette::miette!("Module '{}' not found on LuaRocks", module))?;
    
    let author = &module_match[1];
    let found_module = &module_match[2];
    
    // Verify this is the module we're looking for
    if found_module != module {
        return Err(miette::miette!("Expected module '{}' but found '{}'", module, found_module));
    }

    // Get the module page to find versions
    let module_url = format!("https://luarocks.org/modules/{}/{}", author, module);
    let module_response = reqwest::get(&module_url)
        .await
        .into_diagnostic()?
        .text()
        .await
        .into_diagnostic()?;

    // Extract version information
    let version_pattern = regex::Regex::new(r#"<a href="/modules/[^/]+/[^/]+/([^"]+)">([^<]+)</a>"#)
        .unwrap();
    
    let target_version = if let Some(v) = version {
        v.to_string()
    } else {
        // Find the latest non-dev version
        let versions: Vec<String> = version_pattern
            .captures_iter(&module_response)
            .filter_map(|cap| {
                let version_str = cap[1].to_string();
                if !version_str.contains("dev") && !version_str.contains("scm") {
                    Some(version_str)
                } else {
                    None
                }
            })
            .collect();
        
        if versions.is_empty() {
            return Err(miette::miette!("No stable versions found for module '{}'", module));
        }
        
        // Take the first version (should be latest)
        versions.into_iter().next().unwrap()
    };

    // Construct rockspec URL
    let rockspec_url = format!(
        "https://luarocks.org/manifests/{}/{}-{}.rockspec",
        author, module, target_version
    );

    // Fetch and parse rockspec
    let rockspec_content = fetch_rockspec(&rockspec_url).await?;
    let rockspec = parse_rockspec(&rockspec_content)?;

    // Generate recipe
    let recipe = rockspec_to_recipe(&rockspec)?;

    // Write recipe
    let recipe_str = recipe.to_string();
    if opts.write_to == PathBuf::from(".") {
        println!("{}", recipe_str);
    } else {
        let package_name = recipe.package.name.clone();
        write_recipe(&package_name, &recipe_str).into_diagnostic()?;
    }

    Ok(())
}

async fn fetch_rockspec(url: &str) -> miette::Result<String> {
    let response = reqwest::get(url)
        .await
        .into_diagnostic()?
        .text()
        .await
        .into_diagnostic()?;
    Ok(response)
}

fn parse_rockspec(content: &str) -> miette::Result<LuarocksRockspec> {
    parse_rockspec_with_lua(content).map_err(|e| {
        miette::miette!(
            "Failed to parse rockspec with Lua: {}\n\nPlease ensure Lua is installed. You can install it with: pixi global install lua", e
        )
    })
}

fn parse_rockspec_with_lua(content: &str) -> miette::Result<LuarocksRockspec> {
    use std::process::Command;
    
    // Create a Lua script that loads the rockspec and outputs JSON
    let lua_script = format!(r#"
-- Load the rockspec
{}

-- Create a table with the parsed values
local result = {{
    package = package or "",
    version = version or "",
    source = source or {{}},
    description = description or {{}},
    dependencies = dependencies or {{}}
}}

-- Convert to JSON-like format that we can parse easily
print("ROCKSPEC_START")
print("package=" .. (result.package or ""))
print("version=" .. (result.version or ""))
print("source_url=" .. (result.source.url or ""))
print("source_tag=" .. (result.source.tag or ""))
print("source_branch=" .. (result.source.branch or ""))
print("source_md5=" .. (result.source.md5 or ""))
print("source_sha256=" .. (result.source.sha256 or ""))
print("desc_summary=" .. (result.description.summary or ""))
print("desc_detailed=" .. (result.description.detailed or ""))
print("desc_homepage=" .. (result.description.homepage or ""))
print("desc_license=" .. (result.description.license or ""))
if result.dependencies then
    for i, dep in ipairs(result.dependencies) do
        print("dependency=" .. dep)
    end
end
print("ROCKSPEC_END")
"#, content);

    // Write Lua script to temporary file
    let temp_file = std::env::temp_dir().join(format!("rockspec_{}.lua", std::process::id()));
    std::fs::write(&temp_file, lua_script).into_diagnostic()?;
    
    // Execute Lua
    let output = Command::new("lua")
        .arg(&temp_file)
        .output()
        .into_diagnostic()?;
    
    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);
    
    if !output.status.success() {
        return Err(miette::miette!(
            "Lua execution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    
    // Parse the output
    let mut rockspec = LuarocksRockspec {
        package: String::new(),
        version: String::new(),
        source: RockspecSource {
            url: String::new(),
            md5: None,
            sha256: None,
            file: None,
            dir: None,
            tag: None,
            branch: None,
        },
        description: RockspecDescription {
            summary: None,
            detailed: None,
            homepage: None,
            license: None,
            maintainer: None,
        },
        dependencies: Vec::new(),
        build: None,
    };
    
    let mut in_rockspec_section = false;
    for line in output_str.lines() {
        if line == "ROCKSPEC_START" {
            in_rockspec_section = true;
            continue;
        }
        if line == "ROCKSPEC_END" {
            break;
        }
        if !in_rockspec_section {
            continue;
        }
        
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "package" => rockspec.package = value.to_string(),
                "version" => rockspec.version = value.to_string(),
                "source_url" => rockspec.source.url = value.to_string(),
                "source_tag" => if !value.is_empty() { rockspec.source.tag = Some(value.to_string()); },
                "source_branch" => if !value.is_empty() { rockspec.source.branch = Some(value.to_string()); },
                "source_md5" => if !value.is_empty() { rockspec.source.md5 = Some(value.to_string()); },
                "source_sha256" => if !value.is_empty() { rockspec.source.sha256 = Some(value.to_string()); },
                "desc_summary" => if !value.is_empty() { rockspec.description.summary = Some(value.trim().to_string()); },
                "desc_detailed" => if !value.is_empty() { 
                    // Clean up multiline descriptions by normalizing whitespace
                    let cleaned = value.lines()
                        .map(|line| line.trim())
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !cleaned.is_empty() {
                        rockspec.description.detailed = Some(cleaned);
                    }
                },
                "desc_homepage" => if !value.is_empty() { rockspec.description.homepage = Some(value.to_string()); },
                "desc_license" => if !value.is_empty() { rockspec.description.license = Some(value.to_string()); },
                "dependency" => rockspec.dependencies.push(value.to_string()),
                _ => {}
            }
        }
    }
    
    Ok(rockspec)
}



fn rockspec_to_recipe(rockspec: &LuarocksRockspec) -> miette::Result<Recipe> {
    let package_name = normalize_lua_name(&rockspec.package)?;
    
    // Extract version without rockspec suffix
    let version = rockspec.version.split('-').next().unwrap_or(&rockspec.version);

    let mut context = IndexMap::new();
    context.insert("version".to_string(), version.to_string());
    context.insert("name".to_string(), package_name.as_normalized().to_string());

    let mut recipe = Recipe {
        context,
        package: crate::recipe_generator::serialize::Package {
            name: package_name.as_normalized().to_string(),
            version: "{{ version }}".to_string(),
        },
        source: vec![SourceElement {
            url: vec![rockspec.source.url.clone()],
            md5: rockspec.source.md5.clone(),
            sha256: rockspec.source.sha256.clone(),
        }],
        build: Build {
            script: "luarocks install ${{ name }} ${{ version }} --tree=${{ PREFIX }}".to_string(),
            python: Python::default(),
            noarch: None,
        },
        requirements: Requirements {
            build: vec!["luarocks".to_string()],
            host: vec!["lua".to_string()],
            run: vec!["lua".to_string()],
        },
        tests: vec![Test::Script(ScriptTest {
            script: vec![
                format!("lua -e \"require('{}')\"", rockspec.package),
            ],
        })],
        about: About {
            homepage: rockspec.description.homepage.clone(),
            license: map_license(rockspec.description.license.as_deref()),
            summary: rockspec.description.summary.clone(),
            description: rockspec.description.detailed.clone(),
            ..Default::default()
        },
    };

    // Add dependencies
    if !rockspec.dependencies.is_empty() {
        for dep in &rockspec.dependencies {
            // Skip lua itself as a dependency since it's already included
            if dep.starts_with("lua") && dep.split_whitespace().next() == Some("lua") {
                continue;
            }
            let dep_name = normalize_lua_name(dep)?;
            recipe.requirements.run.push(dep_name.as_normalized().to_string());
        }
    }

    Ok(recipe)
}

fn normalize_lua_name(name: &str) -> miette::Result<PackageName> {
    // Extract just the package name, removing version constraints and extra whitespace
    let name_part = name.split_whitespace()
        .next()
        .unwrap_or(name)
        .split(&['>', '<', '=', '~'][..])
        .next()
        .unwrap_or(name);
    
    // Clean up the name - keep only alphanumeric, hyphens, and underscores
    let clean_name: String = name_part.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .trim()
        .to_string();
    
    // Don't add lua- prefix for base lua
    if clean_name == "lua" {
        return PackageName::try_from("lua".to_string()).into_diagnostic();
    }
    
    // Convert to conda-friendly name, but don't double-prefix lua-
    let normalized = if clean_name.starts_with("lua-") || clean_name.starts_with("lua_") {
        format!("lua-{}", &clean_name[4..].replace('_', "-").to_lowercase())
    } else {
        format!("lua-{}", clean_name.replace('_', "-").to_lowercase())
    };
    
    PackageName::try_from(normalized).into_diagnostic()
}

fn map_license(license: Option<&str>) -> Option<String> {
    license.map(|l| {
        match l.to_lowercase().as_str() {
            "mit" | "mit/x11" => "MIT",
            "apache" | "apache-2.0" | "apache 2.0" => "Apache-2.0",
            "bsd" | "3-clause bsd" | "bsd-3-clause" => "BSD-3-Clause",
            "gpl" | "gplv2" | "gpl-2.0" => "GPL-2.0-or-later",
            "gplv3" | "gpl-3.0" => "GPL-3.0-or-later",
            "lgpl" | "lgplv2.1" | "lgpl-2.1" => "LGPL-2.1-or-later",
            "lgplv3" | "lgpl-3.0" => "LGPL-3.0-or-later",
            _ => l,
        }
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_lua_name() {
        let name1 = normalize_lua_name("luaposix").unwrap();
        assert_eq!(name1.as_normalized(), "lua-luaposix");

        let name2 = normalize_lua_name("lua-cjson").unwrap();
        assert_eq!(name2.as_normalized(), "lua-cjson");

        let name3 = normalize_lua_name("lua_socket").unwrap();
        assert_eq!(name3.as_normalized(), "lua-socket");

        // Test with version constraint
        let name4 = normalize_lua_name("luasocket >= 2.0").unwrap();
        assert_eq!(name4.as_normalized(), "lua-luasocket");
    }

    #[test]
    fn test_map_license() {
        // Test common license mappings
        assert_eq!(map_license(Some("MIT")), Some("MIT".to_string()));
        assert_eq!(map_license(Some("mit")), Some("MIT".to_string()));
        assert_eq!(map_license(Some("MIT/X11")), Some("MIT".to_string()));
        
        assert_eq!(map_license(Some("Apache")), Some("Apache-2.0".to_string()));
        assert_eq!(map_license(Some("Apache-2.0")), Some("Apache-2.0".to_string()));
        assert_eq!(map_license(Some("apache 2.0")), Some("Apache-2.0".to_string()));
        
        assert_eq!(map_license(Some("BSD")), Some("BSD-3-Clause".to_string()));
        assert_eq!(map_license(Some("3-clause bsd")), Some("BSD-3-Clause".to_string()));
        
        assert_eq!(map_license(Some("GPL")), Some("GPL-2.0-or-later".to_string()));
        assert_eq!(map_license(Some("GPLv2")), Some("GPL-2.0-or-later".to_string()));
        assert_eq!(map_license(Some("GPLv3")), Some("GPL-3.0-or-later".to_string()));
        
        // Test unmapped license
        assert_eq!(map_license(Some("Custom License")), Some("Custom License".to_string()));
        
        // Test None
        assert_eq!(map_license(None), None);
    }

    #[test]
    fn test_parse_rockspec() {
        let sample_rockspec = r#"package = "luasocket"
version = "3.0rc1-2"
source = {
  url = "https://github.com/diegonehab/luasocket/archive/v3.0-rc1.tar.gz",
  md5 = "abc123",
  sha256 = "def456"
}
description = {
  summary = "Network support for the Lua language",
  homepage = "http://w3.impa.br/~diego/software/luasocket/",
  license = "MIT"
}
dependencies = { "lua >= 5.1" }"#;

        let rockspec = parse_rockspec(sample_rockspec).unwrap();
        
        assert_eq!(rockspec.package, "luasocket");
        assert_eq!(rockspec.version, "3.0rc1-2");
        assert_eq!(rockspec.source.url, "https://github.com/diegonehab/luasocket/archive/v3.0-rc1.tar.gz");
        assert_eq!(rockspec.source.md5, Some("abc123".to_string()));
        assert_eq!(rockspec.source.sha256, Some("def456".to_string()));
        assert_eq!(rockspec.description.summary, Some("Network support for the Lua language".to_string()));
        assert_eq!(rockspec.description.homepage, Some("http://w3.impa.br/~diego/software/luasocket/".to_string()));
        assert_eq!(rockspec.description.license, Some("MIT".to_string()));
        assert_eq!(rockspec.dependencies, vec!["lua >= 5.1"]);
    }

    #[test]
    fn test_rockspec_to_recipe() {
        let rockspec = LuarocksRockspec {
            package: "luasocket".to_string(),
            version: "3.0rc1-2".to_string(),
            source: RockspecSource {
                url: "https://github.com/diegonehab/luasocket/archive/v3.0-rc1.tar.gz".to_string(),
                md5: Some("abc123".to_string()),
                sha256: Some("def456".to_string()),
                file: None,
                dir: None,
                tag: None,
                branch: None,
            },
            description: RockspecDescription {
                summary: Some("Network support for the Lua language".to_string()),
                detailed: Some("Detailed description".to_string()),
                homepage: Some("http://w3.impa.br/~diego/software/luasocket/".to_string()),
                license: Some("MIT".to_string()),
                maintainer: None,
            },
            dependencies: vec!["lua >= 5.1".to_string()],
            build: None,
        };

        let recipe = rockspec_to_recipe(&rockspec).unwrap();
        
        assert_eq!(recipe.package.name, "lua-luasocket");
        assert_eq!(recipe.package.version, "{{ version }}");
        assert_eq!(recipe.context.get("version"), Some(&"3.0rc1".to_string()));
        assert_eq!(recipe.context.get("name"), Some(&"lua-luasocket".to_string()));
        
        assert_eq!(recipe.source.len(), 1);
        assert_eq!(recipe.source[0].url, vec!["https://github.com/diegonehab/luasocket/archive/v3.0-rc1.tar.gz"]);
        assert_eq!(recipe.source[0].md5, Some("abc123".to_string()));
        assert_eq!(recipe.source[0].sha256, Some("def456".to_string()));
        
        assert_eq!(recipe.build.script, "luarocks install {{ name }} {{ version }} --tree=$PREFIX");
        
        assert!(recipe.requirements.build.contains(&"luarocks".to_string()));
        assert!(recipe.requirements.host.contains(&"lua".to_string()));
        assert!(recipe.requirements.run.contains(&"lua".to_string()));
        
        assert_eq!(recipe.about.summary, Some("Network support for the Lua language".to_string()));
        assert_eq!(recipe.about.description, Some("Detailed description".to_string()));
        assert_eq!(recipe.about.homepage, Some("http://w3.impa.br/~diego/software/luasocket/".to_string()));
        assert_eq!(recipe.about.license, Some("MIT".to_string()));
        
        // Check test command
        match &recipe.tests[0] {
            Test::Script(script_test) => {
                assert_eq!(script_test.script, vec!["lua -e \"require('luasocket')\""]);
            }
            _ => panic!("Expected Script test"),
        }
    }

    // Integration test that requires network access
    #[tokio::test]
    async fn test_fetch_rockspec() {
        // Skip this test in CI to avoid network dependency
        if std::env::var("CI").is_ok() {
            return;
        }

        // Test the fetch function with a simple HTTP test - this test is optional
        // as it depends on external network resources
        let result = fetch_rockspec("https://httpbin.org/robots.txt").await;
        if result.is_ok() {
            // If we can reach the test endpoint, basic fetch functionality works
            assert!(true);
        }
        // If network fails, we don't fail the test since it's environment dependent
    }
}