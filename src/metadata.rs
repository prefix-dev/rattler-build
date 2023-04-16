//! All the metadata that makes up a recipe file
use rattler_conda_types::package::EntryPoint;
use rattler_conda_types::NoArchType;
use rattler_conda_types::Platform;
use serde::{Deserialize, Serialize};
use serde_with::formats::PreferOne;
use serde_with::serde_as;
use serde_with::OneOrMany;
use std::collections::BTreeMap;
use std::env;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use url::Url;

use crate::render::dependency_list::DependencyList;
use crate::render::resolved_dependencies::FinalizedDependencies;

/// The requirements at build- and runtime are defined in the `requirements` section of the recipe.
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Requirements {
    /// Requirements at _build_ time are requirements that can
    /// be run on the machine that is executing the build script.
    /// The environment will thus be resolved with the appropriate platform
    /// that is currently running (e.g. on linux-64 it will be resolved with linux-64).
    /// Typically things like compilers, build tools, etc. are installed here.
    #[serde(default)]
    pub build: DependencyList,
    /// Requirements at _host_ time are requirements that the final executable is going
    /// to _link_ against. The environment will be resolved with the target_platform
    /// architecture (e.g. if you build _on_ linux-64 _for_ linux-aarch64, then the
    /// host environment will be resolved with linux-aarch64).
    ///
    /// Typically things like libraries, headers, etc. are installed here.
    #[serde(default)]
    pub host: DependencyList,
    /// Requirements at _run_ time are requirements that the final executable is going
    /// to _run_ against. The environment will be resolved with the target_platform
    /// at runtime.
    #[serde(default)]
    pub run: DependencyList,
    /// Constrains are optional runtime requirements that are used to constrain the
    /// environment that is resolved. They are not installed by default, but when
    /// installed they will have to conform to the constrains specified here.
    #[serde(default)]
    pub constrains: DependencyList,
}

/// Run exports are applied to downstream packages that depend on this package.
#[derive(Serialize, Debug, Default, Clone)]
pub struct RunExports {
    #[serde(default)]
    pub noarch: DependencyList,
    #[serde(default)]
    pub strong: DependencyList,
    #[serde(default)]
    pub strong_constrains: DependencyList,
    #[serde(default)]
    pub weak: DependencyList,
    #[serde(default)]
    pub weak_constrains: DependencyList,
}

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

impl<'de> Deserialize<'de> for RunExports {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum RunExportsData {
            Map(RunExports),
            List(DependencyList),
        }

        struct RunExportsVisitor;

        impl<'de> Visitor<'de> for RunExportsVisitor {
            type Value = RunExportsData;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a list or a map")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut run_exports = RunExports::default();
                while let Some(key) = access.next_key()? {
                    match key {
                        "strong" => run_exports.strong = access.next_value()?,
                        "weak" => run_exports.weak = access.next_value()?,
                        "weak_constrains" => run_exports.weak_constrains = access.next_value()?,
                        "strong_constrains" => {
                            run_exports.strong_constrains = access.next_value()?
                        }
                        "noarch" => run_exports.noarch = access.next_value()?,
                        _ => (),
                    }
                }
                Ok(RunExportsData::Map(run_exports))
            }

            fn visit_seq<S>(self, mut access: S) -> Result<Self::Value, S::Error>
            where
                S: SeqAccess<'de>,
            {
                let weak =
                    Deserialize::deserialize(de::value::SeqAccessDeserializer::new(&mut access))?;
                Ok(RunExportsData::List(weak))
            }
        }

        let run_exports_data = deserializer.deserialize_any(RunExportsVisitor)?;

        match run_exports_data {
            RunExportsData::Map(run_exports) => Ok(run_exports),
            RunExportsData::List(weak) => Ok(RunExports {
                weak,
                ..Default::default()
            }),
        }
    }
}

/// The build options contain information about how to build the package and some additional
/// metadata about the package.
#[serde_as]
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct BuildOptions {
    /// The build number is a number that should be incremented every time the recipe is built.
    #[serde(default)]
    pub number: u64,
    /// The build string is usually set automatically as the hash of the variant configuration.
    /// It's possible to override this by setting it manually, but not recommended.
    pub string: Option<String>,
    /// The build script can be either a list of commands or a path to a script. By
    /// default, the build script is set to `build.sh` or `build.bat` on Unix and Windows respectively.
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub script: Option<Vec<String>>,
    /// A recipe can choose to ignore certain run exports of its dependencies
    pub ignore_run_exports: Option<Vec<String>>,
    /// A recipe can choose to ignore all run exports of coming from some packages
    pub ignore_run_exports_from: Option<Vec<String>>,
    /// The recipe can specify a list of run exports that it provides
    pub run_exports: Option<RunExports>,
    /// A noarch package runs on any platform. It can be either a python package or a generic package.
    #[serde(default = "NoArchType::default")]
    pub noarch: NoArchType,
    /// For a Python noarch package to have executables it is necessary to specify the python entry points.
    /// These contain the name of the executable and the module + function that should be executed.
    #[serde(default)]
    pub entry_points: Vec<EntryPoint>,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct About {
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub home: Option<Vec<Url>>,
    pub license: Option<String>,
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub license_file: Option<Vec<String>>,
    pub license_family: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub doc_url: Option<Vec<Url>>,
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub dev_url: Option<Vec<Url>>,
}

/// Define tests in your recipe that are executed after successfully building the package.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Test {
    /// Try importing a python module as a sanity check
    pub imports: Option<Vec<String>>,
    /// Run a list of given commands
    pub commands: Option<Vec<String>>,
    /// Extra requirements to be installed at test time
    pub requires: Option<Vec<String>>,
    /// Extra files to be copied to the test environment from the source dir (can be globs)
    pub source_files: Option<Vec<String>>,
    /// Extra files to be copied to the test environment from the build dir (can be globs)
    pub files: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Recipe {
    pub context: BTreeMap<String, serde_yaml::Value>,
    pub name: String,
    pub version: String,
    pub source: Vec<Source>,
    #[serde(default)]
    pub build: BuildOptions,
    #[serde(default)]
    pub requirements: Requirements,
    pub about: About,
}

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
#[serde(rename_all = "lowercase")]
pub enum Checksum {
    Sha256(String),
    Md5(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitRev(String);

impl Default for GitRev {
    fn default() -> Self {
        Self(String::from("HEAD"))
    }
}

/// A git source
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitSrc {
    /// Url to the git repository
    pub git_src: Url,

    /// Optionally a revision to checkout, defaults to `HEAD`
    #[serde(default)]
    pub git_rev: GitRev,

    /// Optionally a depth to clone the repository, defaults to `None`
    pub git_depth: Option<u32>,

    /// Optionally patches to apply to the source code
    pub patches: Option<Vec<PathBuf>>,

    /// Optionally a folder name under the `work` directory to place the source code
    pub folder: Option<PathBuf>,
}

/// A url source (usually a tar.gz or tar.bz2 archive). A compressed file
/// will be extracted to the `work` (or `work/<folder>` directory).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UrlSrc {
    /// Url to the source code (usually a tar.gz or tar.bz2 etc. file)
    pub url: Url,

    /// Optionally a checksum to verify the downloaded file
    #[serde(flatten)]
    pub checksum: Checksum,

    /// Patches to apply to the source code
    pub patches: Option<Vec<PathBuf>>,

    /// Optionally a folder name under the `work` directory to place the source code
    pub folder: Option<PathBuf>,
}

/// A local path source. The source code will be copied to the `work`
/// (or `work/<folder>` directory).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PathSrc {
    /// Path to the local source code
    pub path: PathBuf,

    /// Patches to apply to the source code
    pub patches: Option<Vec<PathBuf>>,

    /// Optionally a folder name under the `work` directory to place the source code
    pub folder: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Source {
    Git(GitSrc),
    Url(UrlSrc),
    Path(PathSrc),
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
    /// The root prefix is a legacy directory where the `conda` tool is installed
    pub root_prefix: PathBuf,
    /// The work directory is the directory where the source code is copied to
    pub work_dir: PathBuf,
    /// The parent directory of host, build and work directories
    pub build_dir: PathBuf,
    /// The output directory or local channel directory
    pub local_channel: PathBuf,
}

fn setup_build_dir(name: &str) -> Result<PathBuf, std::io::Error> {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    let dirname = format!("{}_{:?}", name, since_the_epoch.as_millis());
    let path = env::current_dir()?.join(dirname);
    fs::create_dir_all(path.join("work"))?;
    Ok(path)
}

impl Directories {
    pub fn create(name: &str, recipe_path: &Path) -> Result<Directories, std::io::Error> {
        let build_dir = setup_build_dir(name).expect("Could not create build directory");
        let recipe_dir = recipe_path.parent().unwrap().to_path_buf();

        let output_dir = std::env::var("CONDA_BLD_PATH").unwrap_or("./output".into());
        let output_dir = PathBuf::from(output_dir);
        if !output_dir.exists() {
            fs::create_dir(&output_dir)?;
        }

        let mamba_root_prefix = std::env::var("MAMBA_ROOT_PREFIX").unwrap_or("./micromamba".into());
        let mamba_root_prefix = PathBuf::from(mamba_root_prefix);

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
            root_prefix: mamba_root_prefix,
            recipe_dir,
            local_channel: fs::canonicalize(output_dir)?,
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
    pub subpackages: BTreeMap<String, PackageIdentifier>,
}

impl BuildConfiguration {
    /// true if the build is cross-compiling
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Package {
    /// The name of the package
    pub name: String,
    /// The version of the package
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIdentifier {
    pub name: String,
    pub version: String,
    pub build_string: String,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RenderedRecipe {
    /// Information about the package
    pub package: Package,
    /// The source section of the recipe
    #[serde_as(deserialize_as = "Option<OneOrMany<_, PreferOne>>")]
    pub source: Option<Vec<Source>>,
    /// The build section of the recipe
    #[serde(default)]
    pub build: BuildOptions,
    /// The requirements section of the recipe
    pub requirements: Requirements,
    /// The about section of the recipe
    pub about: About,
    /// The test section of the recipe
    pub test: Option<Test>,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub recipe: RenderedRecipe,
    pub build_configuration: BuildConfiguration,
    pub finalized_dependencies: Option<FinalizedDependencies>,
}

impl Output {
    pub fn name(&self) -> &str {
        &self.recipe.package.name
    }

    pub fn version(&self) -> &str {
        &self.recipe.package.version
    }

    pub fn build_string(&self) -> &str {
        self.recipe.build.string.as_ref().unwrap()
    }
}

impl Display for Output {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\nOutput: {}-{}-{}\n",
            self.name(),
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
