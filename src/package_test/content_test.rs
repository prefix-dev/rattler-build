use crate::package_test::TestError;
use crate::recipe::parser::PackageContents;
use globset::{Glob, GlobSet};
use rattler_conda_types::{package::PathsJson, Platform};

impl PackageContents {
    pub fn include_as_globs(
        &self,
        target_platform: &Platform,
    ) -> Result<Vec<GlobSet>, globset::Error> {
        self.include
            .iter()
            .map(|include| {
                if target_platform.is_windows() {
                    format!("Library/include/{include}")
                } else {
                    format!("include/{include}")
                }
            })
            .map(|include| GlobSet::builder().add(Glob::new(&include)?).build())
            .collect::<Result<Vec<GlobSet>, globset::Error>>()
    }

    pub fn bin_as_globs(&self, target_platform: &Platform) -> Result<Vec<GlobSet>, globset::Error> {
        self.bin
            .iter()
            .map(|bin| {
                if target_platform.is_windows() {
                    GlobSet::builder()
                        .add(Glob::new(bin)?)
                        .add(Glob::new(&format!("Library/mingw-w64/bin/{bin}"))?)
                        .add(Glob::new(&format!("Library/usr/bin/{bin}"))?)
                        .add(Glob::new(&format!("Library/bin/{bin}"))?)
                        .add(Glob::new(&format!("Scripts/{bin}"))?)
                        .add(Glob::new(&format!("bin/{bin}"))?)
                        .build()
                } else {
                    GlobSet::builder()
                        .add(Glob::new(&format!("bin/{bin}"))?)
                        .build()
                }
            })
            .collect::<Result<Vec<GlobSet>, globset::Error>>()
    }

    pub fn lib_as_globs(&self, target_platform: &Platform) -> Result<Vec<GlobSet>, globset::Error> {
        if target_platform.is_windows() {
            // Windows is special because it requires both a `.dll` and a `.bin` file
            let mut result = Vec::new();
            for lib in &self.lib {
                result.push(
                    GlobSet::builder()
                        .add(Glob::new(&format!("Library/lib/{lib}.dll"))?)
                        .build()?,
                );
                result.push(
                    GlobSet::builder()
                        .add(Glob::new(&format!("Library/bin/{lib}.bin"))?)
                        .build()?,
                );
            }
            Ok(result)
        } else {
            self.lib
                .iter()
                .map(|lib| {
                    if target_platform.is_osx() {
                        if lib.ends_with(".dylib") || lib.ends_with(".a") {
                            GlobSet::builder()
                                .add(Glob::new(&format!("lib/{lib}"))?)
                                .build()
                        } else {
                            GlobSet::builder()
                                .add(Glob::new(&format!("lib/{lib}.dylib"))?)
                                .add(Glob::new(&format!("lib/{lib}.*.dylib"))?)
                                .add(Glob::new(&format!("lib/lib{lib}.dylib"))?)
                                .add(Glob::new(&format!("lib/lib{lib}.*.dylib"))?)
                                .build()
                        }
                    } else if target_platform.is_linux() {
                        if lib.ends_with(".so") || lib.ends_with(".a") {
                            GlobSet::builder()
                                .add(Glob::new(&format!("lib/{lib}"))?)
                                .build()
                        } else {
                            GlobSet::builder()
                                .add(Glob::new(&format!("lib/{lib}.so"))?)
                                .add(Glob::new(&format!("lib/{lib}.*.so"))?)
                                .add(Glob::new(&format!("lib/lib{lib}.so"))?)
                                .add(Glob::new(&format!("lib/lib{lib}.*.so"))?)
                                .build()
                        }
                    } else {
                        // TODO
                        unimplemented!("lib_as_globs for target platform: {:?}", target_platform)
                    }
                })
                .collect::<Result<Vec<GlobSet>, globset::Error>>()
        }
    }
}

/// <!-- TODO: better desc. --> Run package content tests.
/// # Arguments
///
/// * `package_content` : The package content test format struct ref.
///
/// # Returns
///
/// * `Ok(())` if the test was successful
/// * `Err(TestError::TestFailed)` if the test failed
pub async fn run_package_content_test(
    package_content: &PackageContents,
    paths_json: &PathsJson,
    target_platform: &Platform,
) -> Result<(), TestError> {
    // files globset
    let mut file_globs = vec![];
    for file_path in &package_content.files {
        file_globs.push((file_path, globset::Glob::new(file_path)?.compile_matcher()));
    }

    // site packages
    let site_package_path = globset::Glob::new("**/site-packages/**")?.compile_matcher();
    let mut site_packages = vec![];
    for sp in &package_content.site_packages {
        let mut s = String::new();
        s.extend(sp.split('.').flat_map(|s| [s, "/"]));
        s.push_str("/__init__.py");
        site_packages.push((sp, s));
    }

    // binaries
    let binary_dir = if target_platform.is_windows() {
        "**/Library/bin/**"
    } else {
        "**/bin/**"
    };
    let binary_dir = globset::Glob::new(binary_dir)?.compile_matcher();
    let mut binary_names = package_content
        .bin
        .iter()
        .map(|bin| {
            if target_platform.is_windows() {
                bin.to_owned() + ".exe"
            } else {
                bin.to_owned()
            }
        })
        .collect::<Vec<_>>();

    // libraries
    let library_dir = if target_platform.is_windows() {
        "Library"
    } else {
        "lib"
    };
    let mut libraries = vec![];
    for lib in &package_content.lib {
        if target_platform.is_windows() {
            libraries.push((
                lib,
                globset::Glob::new(format!("**/{library_dir}/lib/{lib}.dll").as_str())?
                    .compile_matcher(),
                globset::Glob::new(format!("**/{library_dir}/bin/{lib}.lib").as_str())?
                    .compile_matcher(),
            ));
        } else if target_platform.is_osx() {
            libraries.push((
                lib,
                globset::Glob::new(format!("**/{library_dir}/{lib}.dylib").as_str())?
                    .compile_matcher(),
                globset::Glob::new(format!("**/{library_dir}/{lib}.a").as_str())?.compile_matcher(),
            ));
        } else if target_platform.is_unix() {
            libraries.push((
                lib,
                globset::Glob::new(format!("**/{library_dir}/{lib}.so").as_str())?
                    .compile_matcher(),
                globset::Glob::new(format!("**/{library_dir}/{lib}.a").as_str())?.compile_matcher(),
            ));
        } else {
            return Err(TestError::PackageContentTestFailedStr(
                "Package test on target not supported.",
            ));
        }
    }

    // includes
    let include_path = if target_platform.is_windows() {
        "Library/include/"
    } else {
        "include/"
    };
    let include_path = globset::Glob::new(include_path)?.compile_matcher();
    let mut includes = vec![];
    for include in &package_content.include {
        includes.push((
            include,
            globset::Glob::new(include.as_str())?.compile_matcher(),
        ));
    }

    for path in &paths_json.paths {
        // check if all site_packages present
        if !site_packages.is_empty() && site_package_path.is_match(&path.relative_path) {
            let mut s = None;
            for (i, sp) in site_packages.iter().enumerate() {
                // this checks for exact component level match
                if path.relative_path.ends_with(&sp.1) {
                    s = Some(i);
                    break;
                }
            }
            if let Some(i) = s {
                // can panic, but panic here is unreachable
                site_packages.swap_remove(i);
            }
        }

        // check if all file globs have a match
        if !file_globs.is_empty() {
            let mut s = None;
            for (i, (_, fm)) in file_globs.iter().enumerate() {
                if fm.is_match(&path.relative_path) {
                    s = Some(i);
                    break;
                }
            }
            if let Some(i) = s {
                // can panic, but panic here is unreachable
                file_globs.swap_remove(i);
            }
        }

        // check if all includes have a match
        if !includes.is_empty() && include_path.is_match(&path.relative_path) {
            let mut s = None;
            for (i, inc) in includes.iter().enumerate() {
                if inc.1.is_match(&path.relative_path) {
                    s = Some(i);
                    break;
                }
            }
            if let Some(i) = s {
                // can panic, but panic here is unreachable
                includes.swap_remove(i);
            }
        }

        // check if for all all, either a static or dynamic library have a match
        if !libraries.is_empty() {
            let mut s = None;
            for (i, l) in libraries.iter().enumerate() {
                if l.1.is_match(&path.relative_path) || l.2.is_match(&path.relative_path) {
                    s = Some(i);
                    break;
                }
            }
            if let Some(i) = s {
                // can panic, but panic here is unreachable
                libraries.swap_remove(i);
            }
        }

        // check if all binaries have a match
        if !binary_names.is_empty() && binary_dir.is_match(&path.relative_path) {
            let mut s = None;
            for (i, b) in binary_names.iter().enumerate() {
                // the matches component-wise as b is single level,
                // it just matches the last component
                if path.relative_path.ends_with(b) {
                    s = Some(i);
                    break;
                }
            }
            if let Some(i) = s {
                // can panic, but panic here is unreachable
                binary_names.swap_remove(i);
            }
        }
    }
    let mut error = String::new();
    if !file_globs.is_empty() {
        error.push_str(&format!(
            "Some file glob matches not found in package contents.\n{:?}",
            file_globs
                .into_iter()
                .map(|s| s.0)
                .collect::<Vec<&String>>()
        ));
    }
    if !site_packages.is_empty() {
        if !error.is_empty() {
            error.push('\n');
        }
        error.push_str(&format!(
            "Some site packages not found in package contents.\n{:?}",
            site_packages
                .into_iter()
                .map(|s| s.0)
                .collect::<Vec<&String>>()
        ));
    }
    if !includes.is_empty() {
        if !error.is_empty() {
            error.push('\n');
        }
        error.push_str(&format!(
            "Some includes not found in package contents.\n{:?}",
            includes.into_iter().map(|s| s.0).collect::<Vec<&String>>()
        ));
    }
    if !libraries.is_empty() {
        if !error.is_empty() {
            error.push('\n');
        }
        error.push_str(&format!(
            "Some libraries not found in package contents.\n{:?}",
            libraries.into_iter().map(|s| s.0).collect::<Vec<&String>>()
        ));
    }
    if !binary_names.is_empty() {
        if !error.is_empty() {
            error.push('\n');
        }
        error.push_str(&format!(
            "Some binaries not found in package contents.\n{:?}",
            binary_names
        ));
    }
    if error.is_empty() {
        Ok(())
    } else {
        Err(TestError::PackageContentTestFailed(error))
    }
}

#[cfg(test)]
mod tests {
    use super::PackageContents;
    use rattler_conda_types::Platform;

    #[derive(Debug)]
    enum MatchError {
        NoMatch,
    }

    fn test_glob_matches(globs: Vec<globset::GlobSet>, paths: Vec<&str>) -> Result<(), MatchError> {
        let mut matches = Vec::new();
        for path in paths {
            let mut has_match = false;
            for (idx, glob) in globs.iter().enumerate() {
                if glob.is_match(path) {
                    has_match = true;
                    matches.push((idx, path));
                }
            }

            if !has_match {
                return Err(MatchError::NoMatch);
            }
        }

        Ok(())
    }

    #[test]
    fn test_include_globs() {
        let package_contents = PackageContents {
            include: vec!["foo".into(), "bar".into()],
            ..Default::default()
        };

        let globs = package_contents
            .include_as_globs(&Platform::Linux64)
            .unwrap();

        test_glob_matches(globs, vec!["include/foo", "include/bar"]).unwrap();

        let package_contents = PackageContents {
            include: vec!["foo".into(), "bar".into()],
            ..Default::default()
        };

        let globs = package_contents
            .include_as_globs(&Platform::Linux64)
            .unwrap();

        test_glob_matches(globs, vec!["lib/foo", "asd/bar"]).unwrap_err();
    }
}
