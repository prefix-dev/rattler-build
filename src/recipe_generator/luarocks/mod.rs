//! Implements logic to generate a rattler-build recipe from a LuaRocks rockspec
//! file.

use std::{collections::BTreeMap, io::Write, path::PathBuf};

use clap::Parser;
use indexmap::IndexMap;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::PackageName;
use serde::Deserialize;
use tempfile::NamedTempFile;

use crate::recipe_generator::serialize::{
    About, Build, GitSourceElement, Python, Recipe, Requirements, ScriptTest, SourceElement, Test,
    UrlSourceElement, write_recipe,
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

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct LuarocksRockspec {
    pub package: String,
    pub version: String,
    pub source: RockspecSource,
    pub description: RockspecDescription,
    pub dependencies: Vec<String>,
    pub build: Option<RockspecBuild>,
}

#[allow(unused)]
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

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct RockspecDescription {
    pub summary: Option<String>,
    pub detailed: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub maintainer: Option<String>,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct RockspecBuild {
    #[serde(rename = "type")]
    pub build_type: Option<String>,
    pub modules: Option<BTreeMap<String, serde_json::Value>>,
    pub install: Option<BTreeMap<String, serde_json::Value>>,
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
                format!(
                    "https://luarocks.org/manifests/{}/{}-{}.rockspec",
                    author, module, version
                )
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
    let module_link_pattern =
        regex::Regex::new(r#"<a class="title" href="/modules/([^/]+)/([^"]+)">([^<]+)</a>"#)
            .unwrap();

    let module_match = module_link_pattern
        .captures(&response)
        .ok_or_else(|| miette::miette!("Module '{}' not found on LuaRocks", module))?;

    let author = &module_match[1];
    let found_module = &module_match[2];

    // Verify this is the module we're looking for
    if found_module != module {
        return Err(miette::miette!(
            "Expected module '{}' but found '{}'",
            module,
            found_module
        ));
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
    let version_pattern =
        regex::Regex::new(r#"<a href="/modules/[^/]+/[^/]+/([^"]+)">([^<]+)</a>"#).unwrap();

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
            return Err(miette::miette!(
                "No stable versions found for module '{}'",
                module
            ));
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

    let (mut rock_spec_file, rock_spec_file_path) = NamedTempFile::new()
        .into_diagnostic()
        .context("failed to create a temporary file for rockspec")?
        .into_parts();
    rock_spec_file
        .write_all(content.as_bytes())
        .into_diagnostic()
        .context("failed to write rockspec content to temporary file")?;
    drop(rock_spec_file);

    // Create a Lua script that loads the rockspec and outputs JSON
    let lua_script = format!(
        r#"
-- Include a simple library to write json output
{json_lua}

-- Parse the rockspec file
local rockspecFile = "{rockspec_file_path}"
local origPackage = package
local ok, _ = pcall(dofile, rockspecFile)
if not ok then
   error("ERROR: could not load rockspecFile " .. tostring(rockspecFile))
end

-- Resolve name clash
if origPackage == package then
   package = nil
end

-- Output the rockspec in a format that can be parsed by Rust
local out = {{
   rockspec_format=rockspec_format,
   package=package,
   version=version,
   description=description,
   supported_platforms=supported_platforms,
   dependencies=dependencies,
   external_dependencies=external_dependencies,
   source=source,
   build=build,
   modules=modules,
}}
print("ROCKSPEC_START")
print(json.encode(out))
"#,
        json_lua = include_str!("json.lua"),
        rockspec_file_path = if cfg!(windows) {
            rock_spec_file_path.to_string_lossy().replace("\\", "\\\\")
        } else {
            rock_spec_file_path.to_string_lossy().into_owned()
        },
    );

    // Write Lua script to temporary file
    let temp_file = std::env::temp_dir().join(format!("rockspec_{}.lua", std::process::id()));
    fs_err::write(&temp_file, lua_script).into_diagnostic()?;

    // Execute Lua
    let output = Command::new("lua")
        .arg(&temp_file)
        .output()
        .into_diagnostic()?;

    if !output.status.success() {
        return Err(miette::miette!(
            "Lua execution failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Clean up temp file
    let _ = fs_err::remove_file(&temp_file);

    let output_str = String::from_utf8_lossy(&output.stdout);
    let json_section = output_str
        .find("ROCKSPEC_START")
        .map(|pos| output_str.split_at(pos + 14).1)
        .unwrap_or_default()
        .trim();
    if json_section.is_empty() {
        return Err(miette::miette!("No rockspec data found in Lua output"));
    }

    match serde_json::from_str(json_section) {
        Ok(rockspec) => Ok(rockspec),
        Err(e) => Err(miette::miette!(
            "Failed to parse rockspec from Lua output: {e}\n\nOutput:\n{json_section}",
        )),
    }
}

/// Check if a URL is a git repository URL
fn is_git_url(url: &str) -> bool {
    url.contains("git+")
        || url.ends_with(".git")
        || url.starts_with("git://")
        || url.starts_with("git@")
}

fn rockspec_to_recipe(rockspec: &LuarocksRockspec) -> miette::Result<Recipe> {
    let package_name = normalize_lua_name(&rockspec.package)?;

    // Extract version without rockspec suffix
    let version = rockspec
        .version
        .split('-')
        .next()
        .unwrap_or(&rockspec.version);

    let mut context = IndexMap::new();
    context.insert("version".to_string(), version.to_string());
    context.insert("name".to_string(), package_name.as_normalized().to_string());

    // Determine source type and create appropriate SourceElement
    let source_element: SourceElement = if is_git_url(&rockspec.source.url) {
        // Git source
        let mut git_source = GitSourceElement {
            git: rockspec.source.url.clone(),
            branch: rockspec.source.branch.clone(),
            tag: rockspec.source.tag.clone(),
        };
        // We need to strip the "git+" prefix if it exists
        if let Some(url) = rockspec.source.url.strip_prefix("git+") {
            git_source.git = url.to_string();
        }
        git_source.into()
    } else {
        // Regular URL source
        UrlSourceElement {
            url: vec![rockspec.source.url.clone()],
            md5: rockspec.source.md5.clone(),
            sha256: rockspec.source.sha256.clone(),
        }
        .into()
    };

    let mut recipe = Recipe {
        context,
        package: crate::recipe_generator::serialize::Package {
            name: package_name.as_normalized().to_string(),
            version: "${{ version }}".to_string(),
        },
        source: vec![source_element],
        build: Build {
            script: "# Take the first `rockspec` we find (in non-deterministic places unfortunately)\nROCK=$(find . -name \"*.rockspec\" | sort -n -r | head -n 1)\nluarocks install ${ROCK} --tree=${{ PREFIX }}".to_string(),
            python: Python::default(),
            noarch: None,
        },
        requirements: Requirements {
            build: vec!["luarocks".to_string()],
            host: vec!["lua".to_string()],
            run: vec!["lua".to_string()],
        },
        tests: vec![generate_require_test(rockspec)],
        about: About {
            homepage: rockspec.description.homepage.clone(),
            license: map_license(rockspec.description.license.as_deref()),
            summary: rockspec.description.summary.as_deref().map(str::trim).map(ToOwned::to_owned).clone(),
            description: rockspec.description.detailed.as_deref().map(str::trim).map(ToOwned::to_owned).clone(),
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
            recipe
                .requirements
                .run
                .push(dep_name.as_normalized().to_string());
        }
    }

    Ok(recipe)
}

/// Generates a `Test` for the recipe that tries to `require(..)` all modules
/// defined in the rockspec.
fn generate_require_test(spec: &LuarocksRockspec) -> Test {
    // Try to get module names from the build.modules field if present
    let mut modules = Vec::new();
    if let Some(build) = &spec.build {
        if let Some(mods) = &build.modules {
            modules.extend(mods.keys().cloned());
        }
    }
    // If no modules found, fall back to the package name
    if modules.is_empty() {
        modules.push(spec.package.clone());
    }
    // Generate a lua require test for each module
    let script = modules
        .into_iter()
        .map(|m| format!("lua -e \"require('{}')\"", m))
        .collect();
    Test::Script(ScriptTest { script })
}

fn normalize_lua_name(name: &str) -> miette::Result<PackageName> {
    // Extract just the package name, removing version constraints and extra
    // whitespace
    let name_part = name
        .split_whitespace()
        .next()
        .unwrap_or(name)
        .split(&['>', '<', '=', '~'][..])
        .next()
        .unwrap_or(name);

    // Clean up the name - keep only alphanumeric, hyphens, and underscores
    let clean_name: String = name_part
        .chars()
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
        assert_eq!(
            map_license(Some("Apache-2.0")),
            Some("Apache-2.0".to_string())
        );
        assert_eq!(
            map_license(Some("apache 2.0")),
            Some("Apache-2.0".to_string())
        );

        assert_eq!(map_license(Some("BSD")), Some("BSD-3-Clause".to_string()));
        assert_eq!(
            map_license(Some("3-clause bsd")),
            Some("BSD-3-Clause".to_string())
        );

        assert_eq!(
            map_license(Some("GPL")),
            Some("GPL-2.0-or-later".to_string())
        );
        assert_eq!(
            map_license(Some("GPLv2")),
            Some("GPL-2.0-or-later".to_string())
        );
        assert_eq!(
            map_license(Some("GPLv3")),
            Some("GPL-3.0-or-later".to_string())
        );

        // Test unmapped license
        assert_eq!(
            map_license(Some("Custom License")),
            Some("Custom License".to_string())
        );

        // Test None
        assert_eq!(map_license(None), None);
    }

    #[test]
    fn test_parse_rockspec() {
        // Check if `lua` is on the PATH
        if std::process::Command::new("lua").output().is_err() {
            eprintln!("Lua is not installed or not on the PATH. Skipping rockspec parsing test.");
            return;
        }

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
        assert_eq!(
            rockspec.source.url,
            "https://github.com/diegonehab/luasocket/archive/v3.0-rc1.tar.gz"
        );
        assert_eq!(rockspec.source.md5, Some("abc123".to_string()));
        assert_eq!(rockspec.source.sha256, Some("def456".to_string()));
        assert_eq!(
            rockspec.description.summary,
            Some("Network support for the Lua language".to_string())
        );
        assert_eq!(
            rockspec.description.homepage,
            Some("http://w3.impa.br/~diego/software/luasocket/".to_string())
        );
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
        assert_eq!(recipe.package.version, "${{ version }}");
        assert_eq!(recipe.context.get("version"), Some(&"3.0rc1".to_string()));
        assert_eq!(
            recipe.context.get("name"),
            Some(&"lua-luasocket".to_string())
        );

        assert_eq!(recipe.source.len(), 1);
        match &recipe.source[0] {
            SourceElement::Url(url_source) => {
                assert_eq!(
                    url_source.url,
                    vec!["https://github.com/diegonehab/luasocket/archive/v3.0-rc1.tar.gz"]
                );
                assert_eq!(url_source.md5, Some("abc123".to_string()));
                assert_eq!(url_source.sha256, Some("def456".to_string()));
            }
            SourceElement::Git(git_source) => {
                panic!("Expected URL source, got Git source: {:?}", git_source);
            }
        }

        assert!(recipe.build.script.contains("luarocks install"),);

        assert!(recipe.requirements.build.contains(&"luarocks".to_string()));
        assert!(recipe.requirements.host.contains(&"lua".to_string()));
        assert!(recipe.requirements.run.contains(&"lua".to_string()));

        assert_eq!(
            recipe.about.summary,
            Some("Network support for the Lua language".to_string())
        );
        assert_eq!(
            recipe.about.description,
            Some("Detailed description".to_string())
        );
        assert_eq!(
            recipe.about.homepage,
            Some("http://w3.impa.br/~diego/software/luasocket/".to_string())
        );
        assert_eq!(recipe.about.license, Some("MIT".to_string()));

        // Check test command
        match &recipe.tests[0] {
            Test::Script(script_test) => {
                assert_eq!(script_test.script, vec!["lua -e \"require('luasocket')\""]);
            }
            _ => panic!("Expected Script test"),
        }
    }

    #[test]
    fn test_generate_require_test_with_modules() {
        use std::collections::BTreeMap;
        let mut modules = BTreeMap::new();
        modules.insert("mod1".to_string(), serde_json::json!({}));
        modules.insert("mod2".to_string(), serde_json::json!({}));
        let rockspec = LuarocksRockspec {
            package: "somepkg".to_string(),
            version: "1.0-1".to_string(),
            source: RockspecSource {
                url: "https://example.com/somepkg-1.0.tar.gz".to_string(),
                md5: None,
                sha256: None,
                file: None,
                dir: None,
                tag: None,
                branch: None,
            },
            description: RockspecDescription {
                summary: Some("desc".to_string()),
                detailed: None,
                homepage: None,
                license: None,
                maintainer: None,
            },
            dependencies: vec![],
            build: Some(RockspecBuild {
                build_type: Some("builtin".to_string()),
                modules: Some(modules),
                install: None,
            }),
        };
        let test = generate_require_test(&rockspec);
        match test {
            Test::Script(script_test) => {
                assert_eq!(
                    script_test.script,
                    vec!["lua -e \"require('mod1')\"", "lua -e \"require('mod2')\""]
                );
            }
            _ => panic!("Expected Script test"),
        }
    }

    #[test]
    fn test_is_git_url() {
        // Git URLs that should be detected
        assert!(is_git_url("https://github.com/user/repo.git"));
        assert!(is_git_url("https://gitlab.com/user/repo.git"));
        assert!(is_git_url("git://github.com/user/repo.git"));
        assert!(is_git_url("git@github.com:user/repo.git"));
        assert!(is_git_url("git+https://github.com/user/repo.git"));

        // Regular URLs that should not be detected as git
        assert!(!is_git_url("https://example.com/file.tar.gz"));
        assert!(!is_git_url(
            "https://pypi.org/packages/source/p/package/package-1.0.tar.gz"
        ));
        assert!(!is_git_url("ftp://ftp.example.com/file.zip"));
    }

    #[test]
    fn test_git_source_conversion() {
        let rockspec = LuarocksRockspec {
            package: "lua-cjson".to_string(),
            version: "2.1.0-1".to_string(),
            source: RockspecSource {
                url: "https://github.com/mpx/lua-cjson.git".to_string(),
                md5: None,
                sha256: None,
                file: None,
                dir: None,
                tag: Some("2.1.0".to_string()),
                branch: None,
            },
            description: RockspecDescription {
                summary: Some("Fast JSON encoding/parsing".to_string()),
                detailed: None,
                homepage: Some("https://github.com/mpx/lua-cjson".to_string()),
                license: Some("MIT".to_string()),
                maintainer: None,
            },
            dependencies: vec!["lua >= 5.1".to_string()],
            build: None,
        };

        let recipe = rockspec_to_recipe(&rockspec).unwrap();

        // Verify git source structure
        assert_eq!(recipe.source.len(), 1);

        match &recipe.source[0] {
            SourceElement::Url(_) => panic!("Expected Git source, got URL source"),
            SourceElement::Git(git_source) => {
                assert_eq!(
                    git_source.git,
                    "https://github.com/mpx/lua-cjson.git".to_string()
                );
                assert_eq!(git_source.tag, Some("2.1.0".to_string()));
                assert!(git_source.branch.is_none());
            }
        }
    }
}
