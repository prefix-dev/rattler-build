use std::collections::HashSet;
use std::path::PathBuf;

use crate::recipe::parser::PackageContentsTest;
use crate::{metadata::Output, package_test::TestError};
use globset::{Glob, GlobBuilder, GlobSet};
use rattler_conda_types::{Platform, package::PathsJson};

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

/// Section of package contents to build (exists vs not)
#[derive(Clone)]
pub enum Section {
    Include,
    Bin,
    Lib,
    SitePackages,
    Files,
}

impl PackageContentsTest {
    /// Build globs from raw sources
    fn match_files(
        globs: &[crate::recipe::parser::GlobWithSource],
        glob_builder: impl Fn(&str) -> Result<Vec<(String, GlobSet)>, globset::Error>,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        let mut result = Vec::new();
        for glob in globs {
            let globsets = glob_builder(glob.source())?;
            result.extend(globsets);
        }
        Ok(result)
    }

    /// Check a list of (glob, GlobSet) against paths, collecting any missing or forbidden matches
    fn check_globs<'a>(
        globs: &[(String, GlobSet)],
        paths: &[&'a PathBuf],
        section: &str,
        expect_exists: bool,
        collected_issues: &mut Vec<String>,
        matched_paths: &mut HashSet<&'a PathBuf>,
    ) {
        for (glob_str, globset) in globs {
            let matches: Vec<&PathBuf> = paths
                .iter()
                .filter(|p| globset.is_match(p))
                .cloned()
                .collect();
            if expect_exists {
                if !matches.is_empty() {
                    display_success(&matches, glob_str, section);
                    matched_paths.extend(&matches);
                } else {
                    collected_issues.push(format!("No match for {} glob: {}", section, glob_str));
                }
            } else if matches.is_empty() {
                tracing::info!(
                    "{} {} not_exists: \"{}\" check passed - no matching files found",
                    console::style(console::Emoji("✔", "")).green(),
                    section,
                    glob_str
                );
            } else {
                collected_issues.push(format!(
                    "Found matches for {} 'not_exists' glob: {} - files should not exist",
                    section, glob_str
                ));
                for p in matches.iter().take(5) {
                    tracing::error!("  - {}", p.display());
                }
                if matches.len() > 5 {
                    tracing::error!("... and {} more", matches.len() - 5);
                }
            }
        }
    }

    /// Build or not-exists globs for any section in one place
    #[allow(clippy::collapsible_else_if)]
    fn build_section_globs(
        &self,
        section: Section,
        exists: bool,
        target_platform: &Platform,
        version_independent: bool,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        match section {
            Section::Include => {
                let raws = if exists {
                    self.include.exists_globs()
                } else {
                    self.include.not_exists_globs()
                };
                Self::match_files(raws, |source| {
                    let pattern = if target_platform.is_windows() {
                        format!("Library/include/{}", source)
                    } else {
                        format!("include/{}", source)
                    };
                    let globset = GlobSet::builder().add(build_glob(pattern)?).build()?;
                    Ok(vec![(source.to_string(), globset)])
                })
            }
            Section::Bin => {
                let raws = if exists {
                    self.bin.exists_globs()
                } else {
                    self.bin.not_exists_globs()
                };
                Self::match_files(raws, |bin_raw| {
                    let globset = if target_platform.is_windows() {
                        let ext = "{,.exe,.bat,.cmd,.com,.ps1}";
                        GlobSet::builder()
                            .add(build_glob(format!("Library/bin/{bin_raw}{ext}"))?)
                            .add(build_glob(format!("Scripts/{bin_raw}{ext}"))?)
                            .add(build_glob(format!("bin/{bin_raw}{ext}"))?)
                            .add(build_glob(format!("Library/mingw-w64/bin/{bin_raw}{ext}"))?)
                            .add(build_glob(format!("Library/usr/bin/{bin_raw}{ext}"))?)
                            .add(build_glob(format!("{bin_raw}{ext}"))?)
                            .build()
                    } else if matches!(target_platform, &Platform::EmscriptenWasm32) {
                        GlobSet::builder()
                            .add(build_glob(format!("bin/{bin_raw}.js"))?)
                            .add(build_glob(format!("bin/{bin_raw}.wasm"))?)
                            .build()
                    } else {
                        GlobSet::builder()
                            .add(Glob::new(&format!("bin/{bin_raw}"))?)
                            .build()
                    }?;
                    Ok(vec![(bin_raw.to_string(), globset)])
                })
            }
            Section::Lib => {
                let raws = if exists {
                    self.lib.exists_globs()
                } else {
                    self.lib.not_exists_globs()
                };
                if target_platform.is_windows() {
                    Self::match_files(raws, |raw| {
                        let mut res = Vec::new();
                        if raw.ends_with(".dll") {
                            res.push((
                                raw.to_string(),
                                GlobSet::builder()
                                    .add(Glob::new(&format!("Library/bin/{raw}"))?)
                                    .build()?,
                            ));
                        } else if raw.ends_with(".lib") {
                            res.push((
                                raw.to_string(),
                                GlobSet::builder()
                                    .add(Glob::new(&format!("Library/lib/{raw}"))?)
                                    .build()?,
                            ));
                        } else {
                            res.push((
                                raw.to_string(),
                                GlobSet::builder()
                                    .add(Glob::new(&format!("Library/bin/{raw}.dll"))?)
                                    .build()?,
                            ));
                            res.push((
                                raw.to_string(),
                                GlobSet::builder()
                                    .add(Glob::new(&format!("Library/lib/{raw}.lib"))?)
                                    .build()?,
                            ));
                        }
                        Ok(res)
                    })
                } else {
                    Self::match_files(raws, |raw| {
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
                        } else {
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
                        }?;
                        Ok(vec![(raw.to_string(), globset)])
                    })
                }
            }
            Section::SitePackages => {
                let raws = if exists {
                    self.site_packages.exists_globs()
                } else {
                    self.site_packages.not_exists_globs()
                };
                Self::match_files(raws, |source| {
                    let base = if version_independent {
                        "site-packages"
                    } else if target_platform.is_windows() {
                        "Lib/site-packages"
                    } else {
                        "lib/python*/site-packages"
                    };
                    let mut builder = GlobSet::builder();
                    if source.contains('/') {
                        builder.add(build_glob(format!("{base}/{source}"))?);
                    } else {
                        let mut parts = source.split('.').collect::<Vec<_>>();
                        let last = parts.pop().unwrap_or_default();
                        let mut path = parts.join("/");
                        if !path.is_empty() {
                            path.push('/');
                        }
                        builder.add(build_glob(format!("{base}/{path}{last}.py"))?);
                        builder.add(build_glob(format!("{base}/{path}{last}/__init__.py"))?);
                    }
                    let final_set = builder.build()?;
                    Ok(vec![(source.to_string(), final_set)])
                })
            }
            Section::Files => {
                let raws = if exists {
                    self.files.exists_globs()
                } else {
                    self.files.not_exists_globs()
                };
                Self::match_files(raws, |source| {
                    let g = Glob::new(source)?;
                    let set = GlobSet::builder().add(g).build()?;
                    Ok(vec![(source.to_string(), set)])
                })
            }
        }
    }

    /// Retrieve globs for a section
    pub fn get_globs_for_section(
        &self,
        section: Section,
        exists: bool,
        target_platform: &Platform,
        version_independent: bool,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.build_section_globs(section, exists, target_platform, version_independent)
    }

    /// Get include globs that should exist
    pub fn include_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.get_globs_for_section(Section::Include, true, target_platform, false)
    }

    /// Get bin globs that should exist
    pub fn bin_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.get_globs_for_section(Section::Bin, true, target_platform, false)
    }

    /// Get lib globs that should exist
    pub fn lib_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.get_globs_for_section(Section::Lib, true, target_platform, false)
    }

    /// Get site packages globs that should exist
    pub fn site_packages_as_globs(
        &self,
        target_platform: &Platform,
        version_independent: bool,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.get_globs_for_section(
            Section::SitePackages,
            true,
            target_platform,
            version_independent,
        )
    }

    /// Get files globs that should exist
    pub fn files_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.get_globs_for_section(Section::Files, true, target_platform, false)
    }

    /// Get files globs that should not exist
    pub fn files_not_exists_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<(String, GlobSet)>, globset::Error> {
        self.get_globs_for_section(Section::Files, false, target_platform, false)
    }

    /// Run the package content test
    pub fn run_test(&self, paths: &PathsJson, output: &Output) -> Result<(), TestError> {
        let span = tracing::info_span!("Package content test");
        let _enter = span.enter();
        let target_platform = output.target_platform();
        let paths: Vec<&PathBuf> = paths.paths.iter().map(|p| &p.relative_path).collect();

        let mut collected_issues = Vec::new();
        let mut matched_paths = HashSet::<&PathBuf>::new();
        let version_independent = output.recipe.build().is_python_version_independent();

        // Check all sections for both exists and not_exists
        let sections = [
            ("include", Section::Include, false),
            ("bin", Section::Bin, false),
            ("lib", Section::Lib, false),
            ("site_packages", Section::SitePackages, version_independent),
            ("files", Section::Files, false),
        ];

        for (section_name, section, version_independent_override) in sections {
            // Check exists globs
            let globs = self.get_globs_for_section(
                section.clone(),
                true,
                target_platform,
                version_independent_override,
            )?;
            Self::check_globs(
                &globs,
                &paths,
                section_name,
                true,
                &mut collected_issues,
                &mut matched_paths,
            );

            // Check not_exists globs
            let globs = self.get_globs_for_section(
                section,
                false,
                target_platform,
                version_independent_override,
            )?;
            Self::check_globs(
                &globs,
                &paths,
                section_name,
                false,
                &mut collected_issues,
                &mut matched_paths,
            );
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

        if !collected_issues.is_empty() || strict_mode_issue.is_some() {
            tracing::error!("Package content test failed:");

            // Print regular issues first
            for issue in &collected_issues {
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
                collected_issues.push(message.clone());
            }

            return Err(TestError::PackageContentTestFailed(
                collected_issues.join("\n"),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{PackageContentsTest, Section};
    use crate::recipe::parser::GlobCheckerVec;
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
            include: GlobCheckerVec::from_vec(vec!["foo", "bar"], None),
            ..Default::default()
        };

        let globs = package_contents
            .get_globs_for_section(Section::Include, true, &Platform::Linux64, false)
            .unwrap();

        let paths = &["include/foo".to_string(), "include/bar".to_string()];
        test_glob_matches(&globs, paths).unwrap();

        let package_contents = PackageContentsTest {
            include: GlobCheckerVec::from_vec(vec!["foo", "bar"], None),
            ..Default::default()
        };

        let globs = package_contents
            .get_globs_for_section(Section::Include, true, &Platform::Linux64, false)
            .unwrap();

        let paths = &["lib/foo".to_string(), "asd/bar".to_string()];
        test_glob_matches(&globs, paths).unwrap_err();
    }

    #[test]
    fn test_wasm_bin_globs() {
        let package_contents = PackageContentsTest {
            bin: GlobCheckerVec::from_vec(vec!["foo", "bar"], None),
            ..Default::default()
        };

        let globs = package_contents
            .get_globs_for_section(Section::Bin, true, &Platform::EmscriptenWasm32, false)
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
            let globs = tests
                .get_globs_for_section(Section::Include, true, &test_case.platform, false)
                .unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        if !tests.bin.is_empty() {
            println!("bin globs: {:?}", tests.bin);
            let globs = tests
                .get_globs_for_section(Section::Bin, true, &test_case.platform, false)
                .unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        if !tests.lib.is_empty() {
            println!("lib globs: {:?}", tests.lib);
            let globs = tests
                .get_globs_for_section(Section::Lib, true, &test_case.platform, false)
                .unwrap();
            test_glob_matches(&globs, &test_case.paths)?;
            if !test_case.fail_paths.is_empty() {
                test_glob_matches(&globs, &test_case.fail_paths).unwrap_err();
            }
        }

        if !tests.site_packages.is_empty() {
            println!("site_package globs: {:?}", tests.site_packages);
            let globs = tests
                .get_globs_for_section(Section::SitePackages, true, &test_case.platform, false)
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
        let tests = &test_case.package_contents;

        let exists_globs = tests
            .get_globs_for_section(Section::Files, true, &test_case.platform, false)
            .unwrap();
        if !exists_globs.is_empty() {
            test_glob_matches(&exists_globs, &test_case.paths).unwrap();
        }

        let not_exists_globs = tests
            .get_globs_for_section(Section::Files, false, &test_case.platform, false)
            .unwrap();
        if !not_exists_globs.is_empty() && !test_case.fail_paths.is_empty() {
            for (_, glob) in &not_exists_globs {
                for path in &test_case.fail_paths {
                    assert!(glob.is_match(path), "{} should match not_exists glob", path);
                }
            }
        }
    }

    #[test]
    fn test_strict_mode() {
        let strict_contents = PackageContentsTest {
            files: GlobCheckerVec::from_vec(vec!["matched.txt"], None),
            strict: true,
            ..Default::default()
        };
        assert!(strict_contents.strict);

        let non_strict_contents = PackageContentsTest {
            files: GlobCheckerVec::from_vec(vec!["*.txt"], None),
            ..Default::default()
        };
        assert!(!non_strict_contents.strict);
    }
}
