//! Command-line options.

use std::{error::Error, path::PathBuf, str::FromStr};

use clap::{arg, builder::ArgPredicate, crate_version, Parser, ValueEnum};
use clap_complete::{shells, Generator};
use clap_complete_nushell::Nushell;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use rattler_conda_types::{package::ArchiveType, Platform};
use rattler_package_streaming::write::CompressionLevel;
use rattler_solve::ChannelPriority;
use serde_json::{json, Value};
use tracing::warn;
use url::Url;

#[cfg(feature = "recipe-generation")]
use crate::recipe_generator::GenerateRecipeOpts;
use crate::{
    console_utils::{Color, LogStyle},
    script::{SandboxArguments, SandboxConfiguration},
    tool_configuration::{SkipExisting, TestStrategy},
    url_with_trailing_slash::UrlWithTrailingSlash,
};

/// Application subcommands.
#[derive(Parser)]
#[allow(clippy::large_enum_variant)]
pub enum SubCommands {
    /// Build a package from a recipe
    Build(BuildOpts),

    /// Run a test for a single package
    ///
    /// This creates a temporary directory, copies the package file into it, and
    /// then runs the indexing. It then creates a test environment that
    /// installs the package and any extra dependencies specified in the
    /// package test dependencies file.
    ///
    /// With the activated test environment, the packaged test files are run:
    ///
    /// * `info/test/run_test.sh` or `info/test/run_test.bat` on Windows
    /// * `info/test/run_test.py`
    ///
    /// These test files are written at "package creation time" and are part of
    /// the package.
    Test(TestOpts),

    /// Rebuild a package from a package file instead of a recipe.
    Rebuild(RebuildOpts),

    /// Upload a package
    Upload(UploadOpts),

    /// Generate shell completion script
    Completion(ShellCompletion),

    #[cfg(feature = "recipe-generation")]
    /// Generate a recipe from PyPI or CRAN
    GenerateRecipe(GenerateRecipeOpts),

    /// Handle authentication to external channels
    Auth(rattler::cli::auth::Args),
}

/// Shell completion options.
#[derive(Parser)]
pub struct ShellCompletion {
    /// Specifies the shell for which the completions should be generated
    #[arg(short, long)]
    pub shell: Shell,
}

/// Defines the shells for which we can provide completions
#[allow(clippy::enum_variant_names)]
#[derive(ValueEnum, Clone, Debug, Copy, Eq, Hash, PartialEq)]
pub enum Shell {
    /// Bourne Again SHell (bash)
    Bash,
    /// Elvish shell
    Elvish,
    /// Friendly Interactive SHell (fish)
    Fish,
    /// Nushell
    Nushell,
    /// PowerShell
    Powershell,
    /// Z SHell (zsh)
    Zsh,
}

impl Generator for Shell {
    fn file_name(&self, name: &str) -> String {
        match self {
            Shell::Bash => shells::Bash.file_name(name),
            Shell::Elvish => shells::Elvish.file_name(name),
            Shell::Fish => shells::Fish.file_name(name),
            Shell::Nushell => Nushell.file_name(name),
            Shell::Powershell => shells::PowerShell.file_name(name),
            Shell::Zsh => shells::Zsh.file_name(name),
        }
    }

    fn generate(&self, cmd: &clap::Command, buf: &mut dyn std::io::Write) {
        match self {
            Shell::Bash => shells::Bash.generate(cmd, buf),
            Shell::Elvish => shells::Elvish.generate(cmd, buf),
            Shell::Fish => shells::Fish.generate(cmd, buf),
            Shell::Nushell => Nushell.generate(cmd, buf),
            Shell::Powershell => shells::PowerShell.generate(cmd, buf),
            Shell::Zsh => shells::Zsh.generate(cmd, buf),
        }
    }
}

#[allow(missing_docs)]
#[derive(Parser)]
#[clap(version = crate_version!())]
pub struct App {
    /// Subcommand.
    #[clap(subcommand)]
    pub subcommand: Option<SubCommands>,

    /// Enable verbose logging.
    #[command(flatten)]
    pub verbose: Verbosity<InfoLevel>,

    /// Logging style
    #[clap(
        long,
        env = "RATTLER_BUILD_LOG_STYLE",
        default_value = "fancy",
        global = true
    )]
    pub log_style: LogStyle,

    /// Wrap log lines at the terminal width.
    /// This is automatically disabled on CI (by detecting the `CI` environment variable).
    #[clap(
        long,
        env = "RATTLER_BUILD_WRAP_LOG_LINES",
        default_missing_value = "true",
        num_args = 0..=1,
        global = true
    )]
    pub wrap_log_lines: Option<bool>,

    /// Enable or disable colored output from rattler-build.
    /// Also honors the `CLICOLOR` and `CLICOLOR_FORCE` environment variable.
    #[clap(
        long,
        env = "RATTLER_BUILD_COLOR",
        default_value = "auto",
        global = true
    )]
    pub color: Color,
}

impl App {
    /// Returns true if the application will launch a TUI.
    pub fn is_tui(&self) -> bool {
        match &self.subcommand {
            Some(SubCommands::Build(args)) => args.tui,
            _ => false,
        }
    }
}

/// Common opts that are shared between [`Rebuild`] and [`Build`]` subcommands
#[derive(Parser, Clone, Debug)]
pub struct CommonOpts {
    /// Output directory for build artifacts.
    #[clap(
        long,
        env = "CONDA_BLD_PATH",
        verbatim_doc_comment,
        help_heading = "Modifying result"
    )]
    pub output_dir: Option<PathBuf>,

    /// Enable support for repodata.json.zst
    #[clap(long, env = "RATTLER_ZSTD", default_value = "true", hide = true)]
    pub use_zstd: bool,

    /// Enable support for repodata.json.bz2
    #[clap(long, env = "RATTLER_BZ2", default_value = "true", hide = true)]
    pub use_bz2: bool,

    /// Enable experimental features
    #[arg(long, env = "RATTLER_BUILD_EXPERIMENTAL")]
    pub experimental: bool,

    /// Path to an auth-file to read authentication information from
    #[clap(long, env = "RATTLER_AUTH_FILE", hide = true)]
    pub auth_file: Option<PathBuf>,

    /// Channel priority to use when solving
    #[arg(long)]
    pub channel_priority: Option<ChannelPriorityWrapper>,
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct CommonData {
    pub output_dir: PathBuf,
    pub experimental: bool,
    pub auth_file: Option<PathBuf>,
    pub channel_priority: ChannelPriority,
}

impl From<CommonOpts> for CommonData {
    fn from(value: CommonOpts) -> Self {
        Self::new(
            value.output_dir,
            value.experimental,
            value.auth_file,
            value.channel_priority.map(|c| c.value),
        )
    }
}

impl CommonData {
    /// Create a new instance of `CommonData`
    pub fn new(
        output_dir: Option<PathBuf>,
        experimental: bool,
        auth_file: Option<PathBuf>,
        channel_priority: Option<ChannelPriority>,
    ) -> Self {
        Self {
            output_dir: output_dir.unwrap_or_else(|| PathBuf::from("./output")),
            experimental,
            auth_file,
            channel_priority: channel_priority.unwrap_or(ChannelPriority::Strict),
        }
    }
}

/// Container for rattler_solver::ChannelPriority so that it can be parsed
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChannelPriorityWrapper {
    /// The ChannelPriority value to be used when building the Configuration
    pub value: ChannelPriority,
}

impl FromStr for ChannelPriorityWrapper {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "strict" => Ok(ChannelPriorityWrapper {
                value: ChannelPriority::Strict,
            }),
            "disabled" => Ok(ChannelPriorityWrapper {
                value: ChannelPriority::Disabled,
            }),
            _ => Err("Channel priority must be either 'strict' or 'disabled'".to_string()),
        }
    }
}

/// Container for the CLI package format and compression level
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PackageFormatAndCompression {
    /// The archive type that is selected
    pub archive_type: ArchiveType,
    /// The compression level that is selected
    pub compression_level: CompressionLevel,
}

// deserializer for the package format and compression level
impl FromStr for PackageFormatAndCompression {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split(':');
        let package_format = split.next().ok_or("invalid")?;

        let compression = split.next().unwrap_or("default");

        // remove all non-alphanumeric characters
        let package_format = package_format
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>();

        let archive_type = match package_format.to_lowercase().as_str() {
            "tarbz2" => ArchiveType::TarBz2,
            "conda" => ArchiveType::Conda,
            _ => return Err(format!("Unknown package format: {}", package_format)),
        };

        let compression_level = match compression {
            "max" | "highest" => CompressionLevel::Highest,
            "default" | "normal" => CompressionLevel::Default,
            "fast" | "lowest" | "min" => CompressionLevel::Lowest,
            number if number.parse::<i32>().is_ok() => {
                let number = number.parse::<i32>().unwrap_or_default();
                match archive_type {
                    ArchiveType::TarBz2 => {
                        if !(1..=9).contains(&number) {
                            return Err("Compression level for .tar.bz2 must be between 1 and 9"
                                .to_string());
                        }
                    }
                    ArchiveType::Conda => {
                        if !(-7..=22).contains(&number) {
                            return Err(
                                "Compression level for conda packages (zstd) must be between -7 and 22".to_string()
                            );
                        }
                    }
                }
                CompressionLevel::Numeric(number)
            }
            _ => return Err(format!("Unknown compression level: {}", compression)),
        };

        Ok(PackageFormatAndCompression {
            archive_type,
            compression_level,
        })
    }
}

/// Build options.
#[derive(Parser, Clone)]
pub struct BuildOpts {
    /// The recipe file or directory containing `recipe.yaml`. Defaults to the
    /// current directory.
    #[arg(
        short,
        long = "recipe",
        default_value = ".",
        default_value_if("recipe_dir", ArgPredicate::IsPresent, None)
    )]
    pub recipes: Vec<PathBuf>,

    /// The directory that contains recipes.
    #[arg(long, value_parser = is_dir)]
    pub recipe_dir: Option<PathBuf>,

    /// Build recipes up to the specified package.
    #[arg(long)]
    pub up_to: Option<String>,

    /// The build platform to use for the build (e.g. for building with
    /// emulation, or rendering).
    #[arg(long)]
    pub build_platform: Option<Platform>,

    /// The target platform for the build.
    #[arg(long)]
    pub target_platform: Option<Platform>,

    /// The host platform for the build. If set, it will be used to determine
    /// also the target_platform (as long as it is not noarch).
    #[arg(long)]
    pub host_platform: Option<Platform>,

    /// Add a channel to search for dependencies in.
    #[arg(short = 'c', long = "channel")]
    pub channels: Option<Vec<String>>,

    /// Variant configuration files for the build.
    #[arg(short = 'm', long)]
    pub variant_config: Option<Vec<PathBuf>>,

    /// Do not read the `variants.yaml` file next to a recipe.
    #[arg(long)]
    pub ignore_recipe_variants: bool,

    /// Render the recipe files without executing the build.
    #[arg(long)]
    pub render_only: bool,

    /// Render the recipe files with solving dependencies.
    #[arg(long, requires("render_only"))]
    pub with_solve: bool,

    /// Keep intermediate build artifacts after the build.
    #[arg(long)]
    pub keep_build: bool,

    /// Don't use build id(timestamp) when creating build directory name.
    #[arg(long)]
    pub no_build_id: bool,

    /// The package format to use for the build. Can be one of `tar-bz2` or
    /// `conda`. You can also add a compression level to the package format,
    /// e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to
    /// 22).
    #[arg(long, help_heading = "Modifying result", verbatim_doc_comment)]
    pub package_format: Option<PackageFormatAndCompression>,

    #[arg(long)]
    /// The number of threads to use for compression (only relevant when also
    /// using `--package-format conda`)
    pub compression_threads: Option<u32>,

    /// Don't store the recipe in the final package
    #[arg(long, help_heading = "Modifying result")]
    pub no_include_recipe: bool,

    /// Do not run tests after building (deprecated, use `--test=skip` instead)
    #[arg(long, help_heading = "Modifying result", hide = true)]
    pub no_test: bool,

    /// The strategy to use for running tests
    #[arg(long, help_heading = "Modifying result")]
    pub test: Option<TestStrategy>,

    /// Don't force colors in the output of the build script
    #[arg(long, default_value = "true", help_heading = "Modifying result")]
    pub color_build_log: bool,

    #[allow(missing_docs)]
    #[clap(flatten)]
    pub common: CommonOpts,

    /// Launch the terminal user interface.
    #[arg(long, hide = !cfg!(feature = "tui"))]
    pub tui: bool,

    /// Whether to skip packages that already exist in any channel
    /// If set to `none`, do not skip any packages, default when not specified.
    /// If set to `local`, only skip packages that already exist locally,
    /// default when using `--skip-existing. If set to `all`, skip packages
    /// that already exist in any channel.
    #[arg(long, default_missing_value = "local", num_args = 0..=1, help_heading = "Modifying result"
    )]
    pub skip_existing: Option<SkipExisting>,

    /// Define a "noarch platform" for which the noarch packages will be built
    /// for. The noarch builds will be skipped on the other platforms.
    #[arg(long, help_heading = "Modifying result")]
    pub noarch_build_platform: Option<Platform>,

    /// Extra metadata to include in about.json
    #[arg(long, value_parser = parse_key_val)]
    pub extra_meta: Option<Vec<(String, Value)>>,

    #[allow(missing_docs)]
    #[clap(flatten)]
    pub sandbox_arguments: SandboxArguments,
}
#[allow(missing_docs)]
#[derive(Clone, Debug)]
pub struct BuildData {
    pub up_to: Option<String>,
    pub build_platform: Platform,
    pub target_platform: Platform,
    pub host_platform: Platform,
    pub channels: Option<Vec<String>>,
    pub variant_config: Vec<PathBuf>,
    pub ignore_recipe_variants: bool,
    pub render_only: bool,
    pub with_solve: bool,
    pub keep_build: bool,
    pub no_build_id: bool,
    pub package_format: PackageFormatAndCompression,
    pub compression_threads: Option<u32>,
    pub no_include_recipe: bool,
    pub test: TestStrategy,
    pub color_build_log: bool,
    pub common: CommonData,
    pub tui: bool,
    pub skip_existing: SkipExisting,
    pub noarch_build_platform: Option<Platform>,
    pub extra_meta: Option<Vec<(String, Value)>>,
    pub sandbox_configuration: Option<SandboxConfiguration>,
}

impl BuildData {
    /// Creates a new instance of `BuildData`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        up_to: Option<String>,
        build_platform: Option<Platform>,
        target_platform: Option<Platform>,
        host_platform: Option<Platform>,
        channels: Option<Vec<String>>,
        variant_config: Option<Vec<PathBuf>>,
        ignore_recipe_variants: bool,
        render_only: bool,
        with_solve: bool,
        keep_build: bool,
        no_build_id: bool,
        package_format: Option<PackageFormatAndCompression>,
        compression_threads: Option<u32>,
        no_include_recipe: bool,
        test: Option<TestStrategy>,
        common: CommonData,
        tui: bool,
        skip_existing: Option<SkipExisting>,
        noarch_build_platform: Option<Platform>,
        extra_meta: Option<Vec<(String, Value)>>,
        sandbox_configuration: Option<SandboxConfiguration>,
    ) -> Self {
        Self {
            up_to,
            build_platform: build_platform.unwrap_or(Platform::current()),
            target_platform: target_platform
                .or(host_platform)
                .unwrap_or(Platform::current()),
            host_platform: host_platform
                .or(target_platform)
                .unwrap_or(Platform::current()),
            channels,
            variant_config: variant_config.unwrap_or_default(),
            ignore_recipe_variants,
            render_only,
            with_solve,
            keep_build,
            no_build_id,
            package_format: package_format.unwrap_or(PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Default,
            }),
            compression_threads,
            no_include_recipe,
            test: test.unwrap_or_default(),
            color_build_log: true,
            common,
            tui,
            skip_existing: skip_existing.unwrap_or(SkipExisting::None),
            noarch_build_platform,
            extra_meta,
            sandbox_configuration,
        }
    }
}

impl From<BuildOpts> for BuildData {
    fn from(opts: BuildOpts) -> Self {
        Self::new(
            opts.up_to,
            opts.build_platform,
            opts.target_platform,
            opts.host_platform,
            opts.channels,
            opts.variant_config,
            opts.ignore_recipe_variants,
            opts.render_only,
            opts.with_solve,
            opts.keep_build,
            opts.no_build_id,
            opts.package_format,
            opts.compression_threads,
            opts.no_include_recipe,
            opts.test.or(if opts.no_test {
                Some(TestStrategy::Skip)
            } else {
                None
            }),
            opts.common.into(),
            opts.tui,
            opts.skip_existing,
            opts.noarch_build_platform,
            opts.extra_meta,
            opts.sandbox_arguments.into(),
        )
    }
}

fn is_dir(dir: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(dir);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!(
            "Path '{dir}' needs to exist on disk and be a directory",
        ))
    }
}

/// Parse a single key-value pair
fn parse_key_val(s: &str) -> Result<(String, Value), Box<dyn Error + Send + Sync + 'static>> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{}`", s))?;
    Ok((key.to_string(), json!(value)))
}

/// Test options.
#[derive(Parser)]
pub struct TestOpts {
    /// Channels to use when testing
    #[arg(short = 'c', long = "channel")]
    pub channels: Option<Vec<String>>,

    /// The package file to test
    #[arg(short, long)]
    pub package_file: PathBuf,

    /// The number of threads to use for compression.
    #[clap(long, env = "RATTLER_COMPRESSION_THREADS")]
    pub compression_threads: Option<u32>,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct TestData {
    pub channels: Vec<String>,
    pub package_file: PathBuf,
    pub compression_threads: Option<u32>,
    pub common: CommonData,
}

impl From<TestOpts> for TestData {
    fn from(value: TestOpts) -> Self {
        Self::new(
            value.package_file,
            value.channels,
            value.compression_threads,
            value.common.into(),
        )
    }
}

impl TestData {
    /// Create a new instance of `TestData`
    pub fn new(
        package_file: PathBuf,
        channels: Option<Vec<String>>,
        compression_threads: Option<u32>,
        common: CommonData,
    ) -> Self {
        Self {
            package_file,
            channels: channels.unwrap_or(vec!["conda-forge".to_string()]),
            compression_threads,
            common,
        }
    }
}

/// Rebuild options.
#[derive(Parser)]
pub struct RebuildOpts {
    /// The package file to rebuild
    #[arg(short, long)]
    pub package_file: PathBuf,

    /// Do not run tests after building (deprecated, use `--test=skip` instead)
    #[arg(long, hide = true)]
    pub no_test: bool,

    /// The strategy to use for running tests
    #[arg(long, help_heading = "Modifying result")]
    pub test: Option<TestStrategy>,

    /// The number of threads to use for compression.
    #[clap(long, env = "RATTLER_COMPRESSION_THREADS")]
    pub compression_threads: Option<u32>,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct RebuildData {
    pub package_file: PathBuf,
    pub test: TestStrategy,
    pub compression_threads: Option<u32>,
    pub common: CommonData,
}

impl From<RebuildOpts> for RebuildData {
    fn from(value: RebuildOpts) -> Self {
        Self::new(
            value.package_file,
            value.test.unwrap_or(if value.no_test {
                TestStrategy::Skip
            } else {
                TestStrategy::default()
            }),
            value.compression_threads,
            value.common.into(),
        )
    }
}

impl RebuildData {
    /// Create a new instance of `RebuildData`
    pub fn new(
        package_file: PathBuf,
        test: TestStrategy,
        compression_threads: Option<u32>,
        common: CommonData,
    ) -> Self {
        Self {
            package_file,
            test,
            compression_threads,
            common,
        }
    }
}

/// Upload options.
#[derive(Parser, Debug)]
pub struct UploadOpts {
    /// The package file to upload
    #[arg(global = true, required = false)]
    pub package_files: Vec<PathBuf>,

    /// The server type
    #[clap(subcommand)]
    pub server_type: ServerType,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

/// Server type.
#[derive(Clone, Debug, PartialEq, Parser)]
#[allow(missing_docs)]
pub enum ServerType {
    Quetz(QuetzOpts),
    Artifactory(ArtifactoryOpts),
    Prefix(PrefixOpts),
    Anaconda(AnacondaOpts),
    #[clap(hide = true)]
    CondaForge(CondaForgeOpts),
}

/// Upload to a Quetz server.
/// Authentication is used from the keychain / auth-file.
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct QuetzOpts {
    /// The URL to your Quetz server
    #[arg(short, long, env = "QUETZ_SERVER_URL")]
    pub url: Url,

    /// The URL to your channel
    #[arg(short, long = "channel", env = "QUETZ_CHANNEL")]
    pub channels: String,

    /// The Quetz API key, if none is provided, the token is read from the
    /// keychain / auth-file
    #[arg(short, long, env = "QUETZ_API_KEY")]
    pub api_key: Option<String>,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct QuetzData {
    pub url: UrlWithTrailingSlash,
    pub channels: String,
    pub api_key: Option<String>,
}

impl From<QuetzOpts> for QuetzData {
    fn from(value: QuetzOpts) -> Self {
        Self::new(value.url, value.channels, value.api_key)
    }
}

impl QuetzData {
    /// Create a new instance of `QuetzData`
    pub fn new(url: Url, channels: String, api_key: Option<String>) -> Self {
        Self {
            url: url.into(),
            channels,
            api_key,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Parser)]
/// Options for uploading to a Artifactory channel.
/// Authentication is used from the keychain / auth-file.
pub struct ArtifactoryOpts {
    /// The URL to your Artifactory server
    #[arg(short, long, env = "ARTIFACTORY_SERVER_URL")]
    pub url: Url,

    /// The URL to your channel
    #[arg(short, long = "channel", env = "ARTIFACTORY_CHANNEL")]
    pub channels: String,

    /// Your Artifactory username
    #[arg(long, env = "ARTIFACTORY_USERNAME", hide = true)]
    pub username: Option<String>,

    /// Your Artifactory password
    #[arg(long, env = "ARTIFACTORY_PASSWORD", hide = true)]
    pub password: Option<String>,

    /// Your Artifactory token
    #[arg(short, long, env = "ARTIFACTORY_TOKEN")]
    pub token: Option<String>,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct ArtifactoryData {
    pub url: UrlWithTrailingSlash,
    pub channels: String,
    pub token: Option<String>,
}

impl TryFrom<ArtifactoryOpts> for ArtifactoryData {
    type Error = miette::Error;

    fn try_from(value: ArtifactoryOpts) -> Result<Self, Self::Error> {
        let token = match (value.username, value.password, value.token) {
            (_, _, Some(token)) => Some(token),
            (Some(_), Some(password), _) => {
                warn!("Using username and password for Artifactory authentication is deprecated, using password as token. Please use an API token instead.");
                Some(password)
            }
            (Some(_), None, _) => {
                return Err(miette::miette!(
                    "Artifactory username provided without a password"
                ));
            }
            (None, Some(_), _) => {
                return Err(miette::miette!(
                    "Artifactory password provided without a username"
                ));
            }
            _ => None,
        };
        Ok(Self::new(value.url, value.channels, token))
    }
}

impl ArtifactoryData {
    /// Create a new instance of `ArtifactoryData`
    pub fn new(url: Url, channels: String, token: Option<String>) -> Self {
        Self {
            url: url.into(),
            channels,
            token,
        }
    }
}

/// Options for uploading to a prefix.dev server.
/// Authentication is used from the keychain / auth-file
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct PrefixOpts {
    /// The URL to the prefix.dev server (only necessary for self-hosted
    /// instances)
    #[arg(
        short,
        long,
        env = "PREFIX_SERVER_URL",
        default_value = "https://prefix.dev"
    )]
    pub url: Url,

    /// The channel to upload the package to
    #[arg(short, long, env = "PREFIX_CHANNEL")]
    pub channel: String,

    /// The prefix.dev API key, if none is provided, the token is read from the
    /// keychain / auth-file
    #[arg(short, long, env = "PREFIX_API_KEY")]
    pub api_key: Option<String>,

    /// Upload one or more attestation files alongside the package
    /// Note: if you add an attestation, you can _only_ upload a single package.
    #[arg(long, required = false)]
    pub attestation: Option<PathBuf>,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct PrefixData {
    pub url: UrlWithTrailingSlash,
    pub channel: String,
    pub api_key: Option<String>,
    pub attestation: Option<PathBuf>,
}

impl From<PrefixOpts> for PrefixData {
    fn from(value: PrefixOpts) -> Self {
        Self::new(value.url, value.channel, value.api_key, value.attestation)
    }
}

impl PrefixData {
    /// Create a new instance of `PrefixData`
    pub fn new(
        url: Url,
        channel: String,
        api_key: Option<String>,
        attestation: Option<PathBuf>,
    ) -> Self {
        Self {
            url: url.into(),
            channel,
            api_key,
            attestation,
        }
    }
}

/// Options for uploading to a Anaconda.org server
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct AnacondaOpts {
    /// The owner of the distribution (e.g. conda-forge or your username)
    #[arg(short, long, env = "ANACONDA_OWNER")]
    pub owner: String,

    /// The channel / label to upload the package to (e.g. main / rc)
    #[arg(short, long = "channel", env = "ANACONDA_CHANNEL")]
    pub channels: Option<Vec<String>>,

    /// The Anaconda API key, if none is provided, the token is read from the
    /// keychain / auth-file
    #[arg(short, long, env = "ANACONDA_API_KEY")]
    pub api_key: Option<String>,

    /// The URL to the Anaconda server
    #[arg(short, long, env = "ANACONDA_SERVER_URL")]
    pub url: Option<Url>,

    /// Replace files on conflict
    #[arg(long, short, env = "ANACONDA_FORCE")]
    pub force: bool,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct AnacondaData {
    pub owner: String,
    pub channels: Vec<String>,
    pub api_key: Option<String>,
    pub url: UrlWithTrailingSlash,
    pub force: bool,
}

impl From<AnacondaOpts> for AnacondaData {
    fn from(value: AnacondaOpts) -> Self {
        Self::new(
            value.owner,
            value.channels,
            value.api_key,
            value.url,
            value.force,
        )
    }
}

impl AnacondaData {
    /// Create a new instance of `PrefixData`
    pub fn new(
        owner: String,
        channel: Option<Vec<String>>,
        api_key: Option<String>,
        url: Option<Url>,
        force: bool,
    ) -> Self {
        Self {
            owner,
            channels: channel.unwrap_or_else(|| vec!["main".to_string()]),
            api_key,
            url: url
                .unwrap_or_else(|| Url::parse("https://api.anaconda.org").unwrap())
                .into(),
            force,
        }
    }
}

/// Options for uploading to conda-forge
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct CondaForgeOpts {
    /// The Anaconda API key
    #[arg(long, env = "STAGING_BINSTAR_TOKEN")]
    pub staging_token: String,

    /// The feedstock name
    #[arg(long, env = "FEEDSTOCK_NAME")]
    pub feedstock: String,

    /// The feedstock token
    #[arg(long, env = "FEEDSTOCK_TOKEN")]
    pub feedstock_token: String,

    /// The staging channel name
    #[arg(long, env = "STAGING_CHANNEL")]
    pub staging_channel: Option<String>,

    /// The Anaconda Server URL
    #[arg(long, env = "ANACONDA_SERVER_URL")]
    pub anaconda_url: Option<Url>,

    /// The validation endpoint url
    #[arg(long, env = "VALIDATION_ENDPOINT")]
    pub validation_endpoint: Option<Url>,

    /// The CI provider
    #[arg(long, env = "CI")]
    pub provider: Option<String>,

    /// Dry run, don't actually upload anything
    #[arg(long, env = "DRY_RUN")]
    pub dry_run: bool,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct CondaForgeData {
    pub staging_token: String,
    pub feedstock: String,
    pub feedstock_token: String,
    pub staging_channel: String,
    pub anaconda_url: UrlWithTrailingSlash,
    pub validation_endpoint: Url,
    pub provider: Option<String>,
    pub dry_run: bool,
}

impl From<CondaForgeOpts> for CondaForgeData {
    fn from(value: CondaForgeOpts) -> Self {
        Self::new(
            value.staging_token,
            value.feedstock,
            value.feedstock_token,
            value.staging_channel,
            value.anaconda_url,
            value.validation_endpoint,
            value.provider,
            value.dry_run,
        )
    }
}

impl CondaForgeData {
    /// Create a new instance of `PrefixData`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        staging_token: String,
        feedstock: String,
        feedstock_token: String,
        staging_channel: Option<String>,
        anaconda_url: Option<Url>,
        validation_endpoint: Option<Url>,
        provider: Option<String>,
        dry_run: bool,
    ) -> Self {
        Self {
            staging_token,
            feedstock,
            feedstock_token,
            staging_channel: staging_channel.unwrap_or_else(|| "cf-staging".to_string()),
            anaconda_url: anaconda_url
                .unwrap_or_else(|| Url::parse("https://api.anaconda.org").unwrap())
                .into(),
            validation_endpoint: validation_endpoint.unwrap_or_else(|| {
                Url::parse("https://conda-forge.herokuapp.com/feedstock-outputs/copy").unwrap()
            }),
            provider,
            dry_run,
        }
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use rattler_conda_types::package::ArchiveType;
    use rattler_package_streaming::write::CompressionLevel;

    use super::PackageFormatAndCompression;

    #[test]
    fn test_parse_packaging() {
        let package_format = PackageFormatAndCompression::from_str("tar-bz2").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Default
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Default
            }
        );

        let package_format = PackageFormatAndCompression::from_str("tar-bz2:1").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Numeric(1)
            }
        );

        let package_format = PackageFormatAndCompression::from_str(".tar.bz2:max").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Highest
            }
        );

        let package_format = PackageFormatAndCompression::from_str("tarbz2:5").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::TarBz2,
                compression_level: CompressionLevel::Numeric(5)
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:1").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Numeric(1)
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:max").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Highest
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:-5").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Numeric(-5)
            }
        );

        let package_format = PackageFormatAndCompression::from_str("conda:fast").unwrap();
        assert_eq!(
            package_format,
            PackageFormatAndCompression {
                archive_type: ArchiveType::Conda,
                compression_level: CompressionLevel::Lowest
            }
        );
    }
}
