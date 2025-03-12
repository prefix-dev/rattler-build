use std::path::PathBuf;

use crate::recipe::parser::PackageContentsTest;
use crate::{metadata::Output, package_test::TestError};
use globset::{Glob, GlobBuilder, GlobSet};
use rattler_conda_types::{package::PathsJson, Arch, Platform};

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
                    if raw.ends_with(".so")
                        || raw.contains(".so.")
                        || raw.ends_with(".a")
                    {
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
                globset.add(build_glob(format!("{site_packages_base}/{}", site_package.source()))?);
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
            let glob = Glob::new(&file.source())?;
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
        let paths = paths
            .paths
            .iter()
            .map(|p| &p.relative_path)
            .collect::<Vec<_>>();

        let include_globs = self.include_as_globs(target_platform)?;
        let bin_globs = self.bin_as_globs(target_platform)?;
        let lib_globs = self.lib_as_globs(target_platform)?;
        let site_package_globs = self.site_packages_as_globs(
            target_platform,
            output.recipe.build().is_python_version_independent(),
        )?;
        let file_globs = self.files_as_globs()?;

        fn match_glob<'a>(glob: &GlobSet, paths: &'a Vec<&PathBuf>) -> Vec<&'a PathBuf> {
            let mut matches: Vec<&'a PathBuf> = Vec::new();
            for path in paths {
                if glob.is_match(path) {
                    matches.push(path);
                }
            }
            matches
        }

        let mut collected_issues = Vec::new();

        for glob in include_globs {
            let matches = match_glob(&glob.1, &paths);

            if !matches.is_empty() {
                display_success(&matches, &glob.0, "include");
            }

            if matches.is_empty() {
                collected_issues.push(format!("No match for include glob: {}", glob.0));
            }
        }

        for glob in bin_globs {
            let matches = match_glob(&glob.1, &paths);

            if !matches.is_empty() {
                display_success(&matches, &glob.0, "bin");
            }

            if matches.is_empty() {
                collected_issues.push(format!("No match for bin glob: {}", glob.0));
            }
        }

        for glob in lib_globs {
            let matches = match_glob(&glob.1, &paths);

            if !matches.is_empty() {
                display_success(&matches, &glob.0, "lib");
            }

            if matches.is_empty() {
                collected_issues.push(format!("No match for lib glob: {}", glob.0));
            }
        }

        for glob in site_package_globs {
            let matches = match_glob(&glob.1, &paths);

            if !matches.is_empty() {
                display_success(&matches, &glob.0, "site_packages");
            }

            if matches.is_empty() {
                collected_issues.push(format!("No match for site_package glob: {}", glob.0));
            }
        }

        for glob in file_globs {
            let matches = match_glob(&glob.1, &paths);

            if !matches.is_empty() {
                display_success(&matches, &glob.0, "file");
            }

            if matches.is_empty() {
                collected_issues.push(format!("No match for file glob: {}", glob.0));
            }
        }

        if !collected_issues.is_empty() {
            tracing::error!("Package content test failed:");
            for issue in &collected_issues {
                tracing::error!(
                    "- {} {}",
                    console::style(console::Emoji("❌", " ")).red(),
                    issue
                );
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
}
