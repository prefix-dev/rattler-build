//! Jinja functions for recipe evaluation
//!
//! This module provides custom Jinja functions used in rattler-build recipes,
//! such as `compiler()`, `cdt()`, `match()`, etc.

use minijinja::{Environment, Value};
use rattler_conda_types::{Arch, ParseStrictness, Platform, Version, VersionSpec};
use std::collections::HashMap;
use std::str::FromStr;

/// Configuration for Jinja functions (simplified version)
#[derive(Debug, Clone)]
pub struct JinjaConfig {
    /// Target platform for the build
    pub target_platform: Platform,
    /// Build platform
    pub build_platform: Platform,
    /// Host platform (defaults to target_platform if not set)
    pub host_platform: Option<Platform>,
    /// Variant configuration (compiler versions, etc.)
    pub variant: HashMap<String, String>,
}

impl Default for JinjaConfig {
    fn default() -> Self {
        Self {
            target_platform: Platform::current(),
            build_platform: Platform::current(),
            host_platform: None,
            variant: HashMap::new(),
        }
    }
}

/// Get the default compiler for a given language and platform
fn default_compiler(platform: Platform, language: &str) -> Option<String> {
    Some(
        match language {
            // Platform agnostic compilers
            "fortran" => "gfortran",
            lang if !["c", "cxx", "c++"].contains(&lang) => lang,
            // Platform specific compilers
            _ => {
                if platform.is_windows() {
                    match language {
                        "c" => "vs2017",
                        "cxx" | "c++" => "vs2017",
                        _ => unreachable!(),
                    }
                } else if platform.is_osx() {
                    match language {
                        "c" => "clang",
                        "cxx" | "c++" => "clangxx",
                        _ => unreachable!(),
                    }
                } else if matches!(platform, Platform::EmscriptenWasm32) {
                    match language {
                        "c" => "emscripten",
                        "cxx" | "c++" => "emscripten",
                        _ => unreachable!(),
                    }
                } else {
                    match language {
                        "c" => "gcc",
                        "cxx" | "c++" => "gxx",
                        _ => unreachable!(),
                    }
                }
            }
        }
        .to_string(),
    )
}

/// Evaluate compiler function: returns the appropriate compiler for the language
fn compiler_eval(
    lang: &str,
    platform: Platform,
    variant: &HashMap<String, String>,
) -> Result<String, minijinja::Error> {
    let variant_key = format!("{}_compiler", lang);
    let variant_key_version = format!("{}_compiler_version", lang);

    let res = if let Some(name) = variant
        .get(&variant_key)
        .cloned()
        .or_else(|| default_compiler(platform, lang))
    {
        // check if we also have a compiler version
        if let Some(version) = variant.get(&variant_key_version) {
            if version.chars().all(|a| a.is_alphanumeric() || a == '.') {
                Some(format!("{name}_{platform} ={version}"))
            } else {
                Some(format!("{name}_{platform} {version}"))
            }
        } else {
            Some(format!("{name}_{platform}"))
        }
    } else {
        None
    };

    if let Some(res) = res {
        Ok(res)
    } else {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::UndefinedError,
            format!(
                "No compiler found for language: {lang}\nYou should add `{lang}_compiler` to your variant config.",
            ),
        ))
    }
}

/// Setup Jinja environment with rattler-build functions
pub fn setup_jinja_functions(env: &mut Environment, config: &JinjaConfig) {
    let config_clone = config.clone();

    // compiler() function
    env.add_function("compiler", move |lang: String| {
        compiler_eval(&lang, config_clone.target_platform, &config_clone.variant)
    });

    let config_clone2 = config.clone();

    // cdt() function - Core Dependency Tree (for Linux)
    env.add_function("cdt", move |package_name: String| {
        let arch = config_clone2
            .host_platform
            .unwrap_or(config_clone2.target_platform)
            .arch()
            .or_else(|| config_clone2.build_platform.arch());
        let arch_str = arch.map(|arch| format!("{arch}"));

        let cdt_arch = if let Some(s) = config_clone2.variant.get("cdt_arch") {
            s.clone()
        } else {
            match arch {
                Some(Arch::X86) => "i686".to_string(),
                _ => arch_str
                    .as_ref()
                    .ok_or_else(|| {
                        minijinja::Error::new(
                            minijinja::ErrorKind::UndefinedError,
                            "No target or build architecture provided.",
                        )
                    })?
                    .clone(),
            }
        };

        let cdt_name = config_clone2
            .variant
            .get("cdt_name")
            .cloned()
            .unwrap_or_else(|| match arch {
                Some(Arch::S390X | Arch::Aarch64 | Arch::Ppc64le | Arch::Ppc64) => {
                    "cos7".to_string()
                }
                _ => "cos6".to_string(),
            });

        let res = package_name.split_once(' ').map_or_else(
            || format!("{package_name}-{cdt_name}-{cdt_arch}"),
            |(name, ver_build)| format!("{name}-{cdt_name}-{cdt_arch} {ver_build}"),
        );

        Ok(res)
    });

    // match() function - version matching
    env.add_function("match", |a: &Value, spec: &str| {
        if let Some(variant) = a.as_str() {
            // check if version matches spec
            let (version, _) = variant.split_once(' ').unwrap_or((variant, ""));
            // remove trailing .* or *
            let version = version.trim_end_matches(".*").trim_end_matches('*');

            let version = Version::from_str(version).map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::CannotDeserialize,
                    format!("Failed to deserialize `version`: {}", e),
                )
            })?;
            let version_spec =
                VersionSpec::from_str(spec, ParseStrictness::Strict).map_err(|e| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::SyntaxError,
                        format!("Bad syntax for `spec`: {}", e),
                    )
                })?;
            Ok(version_spec.matches(&version))
        } else {
            // if a is undefined, we return true
            Ok(true)
        }
    });

    // Platform check functions
    env.add_function("is_linux", |platform: &str| {
        Ok(Platform::from_str(platform)
            .map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("Invalid platform: {e}"),
                )
            })?
            .is_linux())
    });

    env.add_function("is_osx", |platform: &str| {
        Ok(Platform::from_str(platform)
            .map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("Invalid platform: {e}"),
                )
            })?
            .is_osx())
    });

    env.add_function("is_windows", |platform: &str| {
        Ok(Platform::from_str(platform)
            .map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("Invalid platform: {e}"),
                )
            })?
            .is_windows())
    });

    env.add_function("is_unix", |platform: &str| {
        Ok(Platform::from_str(platform)
            .map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("Invalid platform: {e}"),
                )
            })?
            .is_unix())
    });
}

/// Add default filters to the environment
pub fn setup_default_filters(env: &mut Environment) {
    env.add_filter("version_to_buildstring", |s: String| {
        // we first split the string by whitespace and take the first part
        let s = s.split_whitespace().next().unwrap_or(&s);
        // we then split the string by . and take the first two parts
        let mut parts = s.split('.');
        let major = parts.next().unwrap_or("");
        let minor = parts.next().unwrap_or("");
        format!("{}{}", major, minor)
    });

    env.add_filter("replace", minijinja::filters::replace);
    env.add_filter("lower", minijinja::filters::lower);
    env.add_filter("upper", minijinja::filters::upper);
    env.add_filter("int", minijinja::filters::int);
    env.add_filter("abs", minijinja::filters::abs);
    env.add_filter("bool", minijinja::filters::bool);
    env.add_filter("default", minijinja::filters::default);
    env.add_filter("first", minijinja::filters::first);
    env.add_filter("last", minijinja::filters::last);
    env.add_filter("length", minijinja::filters::length);
    env.add_filter("list", minijinja::filters::list);
    env.add_filter("join", minijinja::filters::join);
    env.add_filter("min", minijinja::filters::min);
    env.add_filter("max", minijinja::filters::max);
    env.add_filter("reverse", minijinja::filters::reverse);
    env.add_filter("sort", minijinja::filters::sort);
    env.add_filter("trim", minijinja::filters::trim);
    env.add_filter("unique", minijinja::filters::unique);
    env.add_filter("split", minijinja::filters::split);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_compiler() {
        let platform = Platform::Linux64;
        assert_eq!("gxx", default_compiler(platform, "cxx").unwrap());
        assert_eq!("gcc", default_compiler(platform, "c").unwrap());
        assert_eq!("gfortran", default_compiler(platform, "fortran").unwrap());

        let platform = Platform::Win64;
        assert_eq!("vs2017", default_compiler(platform, "cxx").unwrap());
        assert_eq!("vs2017", default_compiler(platform, "c").unwrap());

        let platform = Platform::Osx64;
        assert_eq!("clangxx", default_compiler(platform, "cxx").unwrap());
        assert_eq!("clang", default_compiler(platform, "c").unwrap());
    }

    #[test]
    fn test_compiler_eval() {
        let mut variant = HashMap::new();
        let platform = Platform::Linux64;

        // Default compiler
        let result = compiler_eval("c", platform, &variant).unwrap();
        assert_eq!(result, "gcc_linux-64");

        // With compiler version
        variant.insert("c_compiler_version".to_string(), "11".to_string());
        let result = compiler_eval("c", platform, &variant).unwrap();
        assert_eq!(result, "gcc_linux-64 =11");

        // With custom compiler
        variant.insert("c_compiler".to_string(), "clang".to_string());
        let result = compiler_eval("c", platform, &variant).unwrap();
        assert_eq!(result, "clang_linux-64 =11");
    }
}
