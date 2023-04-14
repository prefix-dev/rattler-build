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

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct RunExports {
    #[serde(default)]
    pub strong: Vec<String>,
    #[serde(default)]
    pub weak: Vec<String>,
    #[serde(default)]
    pub weak_constrains: Vec<String>,
    #[serde(default)]
    pub strong_constrains: Vec<String>,
    #[serde(default)]
    pub noarch: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct Directories {
    pub recipe_dir: PathBuf,
    pub host_prefix: PathBuf,
    pub build_prefix: PathBuf,
    pub root_prefix: PathBuf,
    pub source_dir: PathBuf,
    pub work_dir: PathBuf,
    pub build_dir: PathBuf,
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

        let directories = Directories {
            build_dir: build_dir.clone(),
            source_dir: build_dir.join("work"),
            build_prefix: build_dir.join("build_env"),
            host_prefix: build_dir.join("host_env"),
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
    pub target_platform: Platform,
    pub host_platform: Platform,
    pub build_platform: Platform,
    pub variant: BTreeMap<String, String>,
    pub hash: String,
    pub no_clean: bool,
    pub directories: Directories,
    pub channels: Vec<String>,
}

impl BuildConfiguration {
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RenderedRecipe {
    pub package: Package,
    #[serde_as(deserialize_as = "Option<OneOrMany<_, PreferOne>>")]
    pub source: Option<Vec<Source>>,
    #[serde(default)]
    pub build: BuildOptions,
    pub requirements: Requirements,
    pub about: About,
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
}
