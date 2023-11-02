//! All the metadata that makes up a recipe file
use std::{
    collections::BTreeMap,
    env,
    fmt::{self, Display, Formatter},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use rattler_conda_types::{package::ArchiveType, PackageName, Platform};
use serde::{Deserialize, Serialize};

use crate::render::resolved_dependencies::FinalizedDependencies;

pub struct Metadata {
    pub name: String,
    pub version: String,
    pub requirements: Vec<String>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            name: String::from(""),
            version: String::from("0.0.0"),
            requirements: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitRev(String);

impl FromStr for GitRev {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(GitRev(s.to_string()))
    }
}
impl Default for GitRev {
    fn default() -> Self {
        Self(String::from("HEAD"))
    }
}
impl Display for GitRev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Directories used during the build process
#[derive(Debug, Clone)]
pub struct Directories {
    /// The directory where the recipe is located
    pub recipe_dir: PathBuf,
    /// The host prefix is the directory where host dependencies are installed
    /// Exposed as `$PREFIX` (or `%PREFIX%` on Windows) in the build script
    pub host_prefix: PathBuf,
    /// The build prefix is the directory where build dependencies are installed
    /// Exposed as `$BUILD_PREFIX` (or `%BUILD_PREFIX%` on Windows) in the build script
    pub build_prefix: PathBuf,
    /// The work directory is the directory where the source code is copied to
    pub work_dir: PathBuf,
    /// The parent directory of host, build and work directories
    pub build_dir: PathBuf,
    /// The output directory or local channel directory
    pub output_dir: PathBuf,
}

fn setup_build_dir(name: &str, no_build_id: bool) -> Result<PathBuf, std::io::Error> {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    let dirname = if no_build_id {
        format!("rattler-build_{}", name)
    } else {
        format!("rattler-build_{}_{:?}", name, since_the_epoch.as_millis())
    };
    let path = env::temp_dir().join(dirname);
    fs::create_dir_all(path.join("work"))?;
    Ok(path)
}

impl Directories {
    pub fn create(
        name: &str,
        recipe_path: &Path,
        output_dir: &Path,
        no_build_id: bool,
    ) -> Result<Directories, std::io::Error> {
        let build_dir =
            setup_build_dir(name, no_build_id).expect("Could not create build directory");
        let recipe_dir = recipe_path.parent().unwrap().to_path_buf();

        if !output_dir.exists() {
            fs::create_dir(output_dir)?;
        }

        let host_prefix = if cfg!(target_os = "windows") {
            build_dir.join("h_env")
        } else {
            let placeholder_template = "_placehold";
            let mut placeholder = String::new();
            let placeholder_length: usize = 255;

            while placeholder.len() < placeholder_length {
                placeholder.push_str(placeholder_template);
            }

            let placeholder = placeholder
                [0..placeholder_length - build_dir.join("host_env").as_os_str().len()]
                .to_string();

            build_dir.join(format!("host_env{}", placeholder))
        };

        let directories = Directories {
            build_dir: build_dir.clone(),
            build_prefix: build_dir.join("build_env"),
            host_prefix,
            work_dir: build_dir.join("work"),
            recipe_dir,
            output_dir: fs::canonicalize(output_dir)?,
        };

        Ok(directories)
    }
}

#[derive(Debug, Clone)]
pub struct BuildConfiguration {
    /// The target platform for the build
    pub target_platform: Platform,
    /// The host platform (usually target platform, but for `noarch` it's the build platform)
    pub host_platform: Platform,
    /// The build platform (the platform that the build is running on)
    pub build_platform: Platform,
    /// The selected variant for this build
    pub variant: BTreeMap<String, String>,
    /// THe computed hash of the variant
    pub hash: String,
    /// Set to true if the build directories should be kept after the build
    pub no_clean: bool,
    /// The directories for the build (work, source, build, host, ...)
    pub directories: Directories,
    /// The channels to use when resolving environments
    pub channels: Vec<String>,
    /// The timestamp to use for the build
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// All subpackages coming from this output or other outputs from the same recipe
    pub subpackages: BTreeMap<PackageName, PackageIdentifier>,
    /// Package format (.tar.bz2 or .conda)
    pub package_format: ArchiveType,
}

impl BuildConfiguration {
    /// true if the build is cross-compiling
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIdentifier {
    pub name: PackageName,
    pub version: String,
    pub build_string: String,
}

#[derive(Clone)]
pub struct Output {
    pub recipe: crate::recipe::parser::Recipe,
    pub build_configuration: BuildConfiguration,
    pub finalized_dependencies: Option<FinalizedDependencies>,
}

impl Output {
    /// The name of the package
    pub fn name(&self) -> &PackageName {
        self.recipe.package().name()
    }

    /// The version of the package
    pub fn version(&self) -> &str {
        self.recipe.package().version()
    }

    /// The build string is usually set automatically as the hash of the variant configuration.
    pub fn build_string(&self) -> &str {
        self.recipe.build().string().as_ref().unwrap()
    }

    /// retrieve an identifier for this output ({name}-{version}-{build_string})
    pub fn identifier(&self) -> String {
        format!(
            "{}-{}-{}",
            self.name().as_normalized(),
            self.version(),
            self.build_string()
        )
    }
}

impl Display for Output {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\nOutput: {}-{}-{}\n",
            self.name().as_normalized(),
            self.version(),
            self.build_string()
        )?;

        // make a table of the variant configuration
        writeln!(f, "Variant configuration:")?;

        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL)
            .set_header(vec!["Variant", "Version"]);

        self.build_configuration.variant.iter().for_each(|(k, v)| {
            table.add_row(vec![k, v]);
        });

        writeln!(f, "{}\n", table)?;

        if let Some(finalized_dependencies) = &self.finalized_dependencies {
            // create a table with the finalized dependencies
            if let Some(host) = &finalized_dependencies.build {
                writeln!(f, "Build dependencies:")?;
                writeln!(f, "{}\n", host)?;
            }

            if let Some(host) = &finalized_dependencies.host {
                writeln!(f, "Host dependencies:")?;
                writeln!(f, "{}\n", host)?;
            }

            if !finalized_dependencies.run.depends.is_empty() {
                writeln!(f, "Run dependencies:")?;
                let mut table = comfy_table::Table::new();
                table
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
                    .set_header(vec!["Name", "Spec"]);

                finalized_dependencies.run.depends.iter().for_each(|d| {
                    let rendered = d.render();
                    table.add_row(rendered.splitn(2, ' ').collect::<Vec<&str>>());
                });

                writeln!(f, "{}\n", table)?;
            }

            if !finalized_dependencies.run.constrains.is_empty() {
                writeln!(f, "Run constraints:")?;
                let mut table = comfy_table::Table::new();
                table
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
                    .set_header(vec!["Name", "Spec"]);

                finalized_dependencies.run.constrains.iter().for_each(|d| {
                    let rendered = d.render();
                    table.add_row(rendered.splitn(2, ' ').collect::<Vec<&str>>());
                });

                writeln!(f, "{}\n", table)?;
            }
        }
        writeln!(f, "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_build_dir_test() {
        let temp_dir = std::env::temp_dir();
        let temp_dir = temp_dir.to_str().unwrap();

        // without build_id (aka timestamp)
        let p1 = setup_build_dir("name", true).unwrap();
        let ps1 = p1.to_str().unwrap();
        assert!(ps1.eq(&format!("{temp_dir}rattler-build_name")));
        _ = ps1;
        _ = std::fs::remove_dir_all(p1);

        // with build_id (aka timestamp)
        let p2 = setup_build_dir("name", false).unwrap();
        let ps2 = p2.to_str().unwrap();
        assert!(ps2.starts_with(&format!("{temp_dir}rattler-build_name_")));
        _ = ps2;
        _ = std::fs::remove_dir_all(p2);
    }
}
