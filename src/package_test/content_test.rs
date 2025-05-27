use std::collections::HashSet;
use std::path::PathBuf;

use crate::recipe::parser::PackageContentsTest;
use crate::{metadata::Output, package_test::TestError};
use globset::{Glob, GlobBuilder, GlobSet};
use rattler_conda_types::{Arch, Platform, package::PathsJson};

fn build_glob(glob: String) -> Result<Glob, globset::Error> {
    tracing::debug!("Building glob: {}", glob);
    GlobBuilder::new(&glob).empty_alternates(true).build()
}

fn display_success(matches: &[&PathBuf], glob: &str, section: &str) {
    tracing::info!(
        "{} {section}: \"{}\" matched:",
        console::style(console::Emoji("✔", "")).green(),
        glob
    );
    for m in matches[0..std::cmp::min(5, matches.len())].iter() {
        tracing::info!("  - {}", m.display());
    }
    if matches.len() > 5 {
        tracing::info!("... and {} more", matches.len() - 5);
    }
}

impl PackageContentsTest {
    /// Retrieve the include globs as a vector of (glob, GlobSet) tuples
    pub fn include_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        let mut result = Vec::new();
        for include in self.include.include_globs() {
            let glob = if target_platform.is_windows() {
                format!("Library/include/{}", include.source())
            } else {
                format!("include/{}", include.source())
            };

            result.push((
                include.glob().to_string(),
                GlobSet::builder().add(build_glob(glob)?).build()?,
            ));
        }

        Ok(result)
    }

    /// Retrieve the globs for the bin section as a vector of (glob, GlobSet) tuples
    pub fn bin_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        let mut result = Vec::new();

        for bin in self.bin.include_globs() {
            let bin_raw = bin.source();
            let globset = if target_platform.is_windows() {
                // This is usually encoded as `PATHEXT` in the environment
                let path_ext = "{,.exe,.bat,.cmd,.com,.ps1}";
                GlobSet::builder()
                    .add(build_glob(format!("{bin_raw}{path_ext}"))?)
                    .add(build_glob(format!(
                        "Library/mingw-w64/bin/{bin_raw}{path_ext}"
                    ))?)
                    .add(build_glob(format!("Library/usr/bin/{bin_raw}{path_ext}"))?)
                    .add(build_glob(format!("Library/bin/{bin_raw}{path_ext}"))?)
                    .add(build_glob(format!("Scripts/{bin_raw}{path_ext}"))?)
                    .add(build_glob(format!("bin/{bin_raw}{path_ext}"))?)
                    .build()
            } else if matches!(target_platform, &Platform::EmscriptenWasm32) {
                // emscripten build outputs will gonna get .js extension
                GlobSet::builder()
                    .add(build_glob(format!("bin/{bin_raw}.js"))?)
                    .add(build_glob(format!("bin/{bin_raw}.wasm"))?)
                    .build()
            } else {
                GlobSet::builder()
                    .add(Glob::new(&format!("bin/{bin_raw}"))?)
                    .build()
            }?;

            result.push((bin.source().to_string(), globset));
        }

        Ok(result)
    }

    /// Retrieve the globs for the lib section as a vector of (glob, GlobSet) tuples
    pub fn lib_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        let mut result = Vec::new();

        if target_platform.is_windows() {
            // Windows is special because it requires both a `.dll` and a `.bin` file
            for lib in self.lib.include_globs() {
                let raw = lib.source();
                if raw.ends_with(".dll") {
                    result.push((
                        raw.to_string(),
                        GlobSet::builder()
                            .add(Glob::new(&format!("Library/bin/{raw}"))?)
                            .build()?,
                    ));
                } else if raw.ends_with(".lib") {
                    result.push((
                        raw.to_string(),
                        GlobSet::builder()
                            .add(Glob::new(&format!("Library/lib/{raw}"))?)
                            .build()?,
                    ));
                } else {
                    result.push((
                        raw.to_string(),
                        GlobSet::builder()
                            .add(Glob::new(&format!("Library/bin/{raw}.dll"))?)
                            .build()?,
                    ));
                    result.push((
                        raw.to_string(),
                        GlobSet::builder()
                            .add(Glob::new(&format!("Library/lib/{raw}.lib"))?)
                            .build()?,
                    ));
                }
            }
        } else {
            for lib in self.lib.include_globs() {
                let raw = lib.source();
                let globset = if target_platform.is_osx() {
                    if raw.ends_with(".dylib") || raw.ends_with(".a") {
                        GlobSet::builder()
                            .add(Glob::new(&format!("lib/{raw}"))?)
                            .build()
                    } else {
                        GlobSet::builder()
                            .add(build_glob(format!("lib/{{,lib}}{raw}.dylib"))?)
                            .add(build_glob(format!("lib/{{,lib}}{raw}.*.dylib"))?)
                            .build()
                    }
                } else if target_platform.is_linux() || target_platform.arch() == Some(Arch::Wasm32)
                {
                    if raw.ends_with(".so") || raw.contains(".so.") || raw.ends_with(".a") {
                        GlobSet::builder()
                            .add(Glob::new(&format!("lib/{raw}"))?)
                            .build()
                    } else {
                        GlobSet::builder()
                            .add(build_glob(format!("lib/{{,lib}}{raw}.so"))?)
                            .add(build_glob(format!("lib/{{,lib}}{raw}.so.*"))?)
                            .build()
                    }
                } else {
                    // TODO
                    unimplemented!("lib_as_globs for target platform: {:?}", target_platform)
                }?;
                result.push((raw.to_string(), globset));
            }
        }

        Ok(result)
    }

    /// Retrieve the globs for the site_packages section as a vector of (glob, GlobSet) tuples
    pub fn site_packages_as_globs(
        &self,
        target_platform: &Platform,
        version_independent: bool,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        let mut result = Vec::new();

        let site_packages_base = if version_independent {
            "site-packages"
        } else if target_platform.is_windows() {
            "Lib/site-packages"
        } else {
            "lib/python*/site-packages"
        };

        for site_package in self.site_packages.include_globs() {
            let mut globset = GlobSet::builder();

            if site_package.source().contains('/') {
                globset.add(build_glob(format!(
                    "{site_packages_base}/{}",
                    site_package.source()
                ))?);
            } else {
                let mut split = site_package.source().split('.').collect::<Vec<_>>();
                let last_elem = split.pop().unwrap_or_default();
                let mut site_package_path = split.join("/");
                if !site_package_path.is_empty() {
                    site_package_path.push('/');
                }

                globset.add(build_glob(format!(
                    "{site_packages_base}/{site_package_path}{last_elem}.py"
                ))?);
                globset.add(build_glob(format!(
                    "{site_packages_base}/{site_package_path}{last_elem}/__init__.py"
                ))?);
            };

            let globset = globset.build()?;
            result.push((site_package.glob().to_string(), globset));
        }

        Ok(result)
    }

    /// Retrieve the globs for the files section as a vector of (glob, GlobSet) tuples
    pub fn files_as_globs(&self) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        let mut result = Vec::new();

        for file in self.files.include_globs() {
            let glob = Glob::new(file.source())?;
            let globset = GlobSet::builder().add(glob).build()?;
            result.push((file.glob().to_string(), globset));
        }

        Ok(result)
    }

    /// Run the package content test
    pub fn run_test(&self, paths: &PathsJson, output: &Output) -> Result<(), TestError> {
        let span = tracing::info_span!("Package content test");
        let _enter = span.enter();
        let target_platform = output.target_platform();
        let paths: Vec<&PathBuf> = paths.paths.iter().map(|p| &p.relative_path).collect();

        // Collect all glob patterns
        let all_globs = [
            ("include", self.include_as_globs(target_platform)?),
            ("bin", self.bin_as_globs(target_platform)?),
            ("lib", self.lib_as_globs(target_platform)?),
            (
                "site_packages",
                self.site_packages_as_globs(
                    target_platform,
                    output.recipe.build().is_python_version_independent(),
                )?,
            ),
            ("files", self.files_as_globs()?),
        ];

        let mut matched_paths = HashSet::<&PathBuf>::new();
        let mut issues = Vec::new();

        // Check all globs
        for (section, globs) in &all_globs {
            for (glob_str, globset) in globs {
                let matches: Vec<&PathBuf> = paths
                    .iter()
                    .filter(|path| globset.is_match(path))
                    .copied()
                    .collect();

                if matches.is_empty() {
                    issues.push(format!("No match for {} glob: {}", section, glob_str));
                } else {
                    display_success(&matches, glob_str, section);
                    matched_paths.extend(&matches);
                }
            }
        }

        // Check strict mode
        let strict_mode_issue = if self.strict {
            let unmatched: Vec<&PathBuf> = paths
                .iter()
                .filter(|p| !matched_paths.contains(*p))
                .copied()
                .collect();

            if !unmatched.is_empty() {
                Some((
                    format!("Strict mode: {} unmatched files found", unmatched.len()),
                    unmatched,
                ))
            } else {
                None
            }
        } else {
            None
        };

        if !issues.is_empty() || strict_mode_issue.is_some() {
            tracing::error!("Package content test failed:");

            // Print regular issues first
            for issue in &issues {
                tracing::error!(
                    "- {} {}",
                    console::style(console::Emoji("❌", " ")).red(),
                    issue
                );
            }

            // Print strict mode issues if any
            if let Some((message, unmatched)) = &strict_mode_issue {
                tracing::error!("\nStrict mode violations:");
                for file in unmatched {
                    tracing::error!(
                        "- {} {}",
                        console::style(console::Emoji("❌", " ")).red(),
                        file.display()
                    );
                }
                issues.push(message.clone());
            }

            return Err(TestError::PackageContentTestFailed(issues.join("\n")));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::PackageContentsTest;
    use crate::recipe::parser::GlobVec;
    use globset::GlobSet;
    use rattler_conda_types::Platform;
    use serde::Deserialize;

    #[derive(Debug)]
    enum MatchError {
        NoMatch,
    }

    fn test_glob_matches(globs: &[(String, GlobSet)], paths: &[String]) -> Result<(), MatchError> {
        let mut matches = Vec::new();
        for path in paths {
            let mut has_match = false;
            for (idx, glob) in globs.iter().enumerate() {
                if glob.1.is_match(path) {
                    has_match = true;
                    matches.push((idx, path));
                }
            }

            if !has_match {
                println!("No match for path: {}", path);
                return Err(MatchError::NoMatch);
            }
        }

        Ok(())
    }

    #[test]
    fn test_include_globs() {
        let package_contents = PackageContentsTest {
            include: GlobVec::from_vec(vec!["foo", "bar"], None),
            ..Default::default()
        };

        let globs = package_contents
            .include_as_globs(&Platform::Linux64)
            .unwrap();

        let paths = &["include/foo".to_string(), "include/bar".to_string()];
        test_glob_matches(&globs, paths).unwrap();

        let package_contents = PackageContentsTest {
            include: GlobVec::from_vec(vec!["foo", "bar"], None),
            ..Default::default()
        };

        let globs = package_contents
            .include_as_globs(&Platform::Linux64)
            .unwrap();

        let paths = &["lib/foo".to_string(), "asd/bar".to_string()];
        test_glob_matches(&globs, paths).unwrap_err();
    }

    #[test]
    fn test_wasm_bin_globs() {
        let package_contents = PackageContentsTest {
            bin: GlobVec::from_vec(vec!["foo", "bar"], None),
            ..Default::default()
        };

        let globs = package_contents
            .bin_as_globs(&Platform::EmscriptenWasm32)
            .unwrap();

        let paths = &[
            "bin/foo.js".to_string(),
            "bin/bar.js".to_string(),
            "bin/foo.wasm".to_string(),
            "bin/bar.wasm".to_string(),
        ];
        test_glob_matches(&globs, paths).unwrap();

        let bad_paths = &["bin/foo".to_string(), "bin/bar".to_string()];
        test_glob_matches(&globs, bad_paths).unwrap_err();
    }

    #[derive(Debug, Deserialize)]
    struct TestCase {
        platform: Platform,
        package_contents: PackageContentsTest,
        paths: Vec<String>,
        #[serde(default)]
        fail_paths: Vec<String>,
    }

    fn load_test_case(path: &Path) -> TestCase {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/package_content");
        let file = std::fs::File::open(test_data_dir.join(path)).unwrap();
        serde_yaml::from_reader(file).unwrap()
    }

    fn evaluate_test_case(test_case: TestCase) -> Result<(), MatchError> {
        let tests = test_case.package_contents;

        if !tests.include.is_empty() {
            println!("include globs: {:?}", tests.include);
            let globs = tests.include_as_globs(&test_case.platform).unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        if !tests.bin.is_empty() {
            println!("bin globs: {:?}", tests.bin);
            let globs = tests.bin_as_globs(&test_case.platform).unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        if !tests.lib.is_empty() {
            println!("lib globs: {:?}", tests.lib);
            let globs = tests.lib_as_globs(&test_case.platform).unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        if !tests.site_packages.is_empty() {
            println!("site_package globs: {:?}", tests.site_packages);
            let globs = tests
                .site_packages_as_globs(&test_case.platform, false)
                .unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        Ok(())
    }

    #[test]
    fn test_include_globs_yaml() {
        let test_case = load_test_case(Path::new("test_include_unix.yaml"));
        evaluate_test_case(test_case).unwrap();

        let test_case = load_test_case(Path::new("test_include_win.yaml"));
        evaluate_test_case(test_case).unwrap();
    }

    #[test]
    fn test_bin_globs() {
        let test_case = load_test_case(Path::new("test_bin_unix.yaml"));
        evaluate_test_case(test_case).unwrap();

        let test_case = load_test_case(Path::new("test_bin_win.yaml"));
        evaluate_test_case(test_case).unwrap();
    }

    #[test]
    fn test_lib_globs() {
        let test_case = load_test_case(Path::new("test_lib_linux.yaml"));
        evaluate_test_case(test_case).unwrap();

        let test_case = load_test_case(Path::new("test_lib_macos.yaml"));
        evaluate_test_case(test_case).unwrap();

        let test_case = load_test_case(Path::new("test_lib_win.yaml"));
        evaluate_test_case(test_case).unwrap();
    }

    #[test]
    fn test_site_package_globs() {
        let test_case = load_test_case(Path::new("test_site_packages_unix.yaml"));
        evaluate_test_case(test_case).unwrap();

        let test_case = load_test_case(Path::new("test_site_packages_win.yaml"));
        evaluate_test_case(test_case).unwrap();
    }

    #[test]
    fn test_file_globs() {
        let test_case = load_test_case(Path::new("test_files.yaml"));
        evaluate_test_case(test_case).unwrap();
    }

    #[test]
    fn test_strict_mode() {
        let strict_contents = PackageContentsTest {
            files: GlobVec::from_vec(vec!["matched.txt"], None),
            strict: true,
            ..Default::default()
        };
        assert!(strict_contents.strict);

        let non_strict_contents = PackageContentsTest {
            files: GlobVec::from_vec(vec!["*.txt"], None),
            ..Default::default()
        };
        assert!(!non_strict_contents.strict);
    }
}
