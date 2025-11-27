//! Command-line options.

use std::{collections::HashMap, error::Error, path::PathBuf, str::FromStr};

use chrono;
use clap::{Parser, ValueEnum, arg, builder::ArgPredicate, crate_version};
use clap_complete::{Generator, shells};
use clap_complete_nushell::Nushell;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use rattler_conda_types::{
    NamedChannelOrUrl, Platform, compression_level::CompressionLevel, package::ArchiveType,
};
use rattler_config::config::ConfigBase;
use rattler_config::config::build::PackageFormatAndCompression;
use rattler_networking::mirror_middleware;
#[cfg(feature = "s3")]
use rattler_networking::s3_middleware;
use rattler_solve::ChannelPriority;
use rattler_upload::upload::opt::UploadOpts;
use serde_json::{Value, json};
use url::Url;

#[cfg(feature = "recipe-generation")]
use crate::recipe_generator::GenerateRecipeOpts;
use crate::{
    console_utils::{Color, LogStyle},
    metadata::Debug,
    script::{SandboxArguments, SandboxConfiguration},
    tool_configuration::{ContinueOnFailure, SkipExisting, TestStrategy},
};

/// Application subcommands.
#[derive(Parser)]
#[allow(clippy::large_enum_variant)]
pub enum SubCommands {
    /// Build a package from a recipe
    Build(BuildOpts),

    /// Publish packages to a channel.
    /// This command builds packages from recipes (or uses already built packages),
    /// uploads them to a channel, and runs indexing.
    Publish(PublishOpts),

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
    /// Generate a recipe from PyPI, CRAN, CPAN, or LuaRocks
    GenerateRecipe(GenerateRecipeOpts),

    /// Handle authentication to external channels
    Auth(rattler::cli::auth::Args),

    /// Debug a recipe by setting up the environment without running the build script
    Debug(DebugOpts),

    /// Create a patch for a directory
    CreatePatch(CreatePatchOpts),

    /// Open a debug shell in the build environment
    DebugShell(DebugShellOpts),

    /// Package-related subcommands
    #[command(subcommand)]
    Package(PackageCommands),

    /// Bump a recipe to a new version
    ///
    /// This command updates the version and SHA256 checksum(s) in a recipe file.
    /// It can either use a specified version or auto-detect the latest version
    /// from supported providers (GitHub, PyPI, crates.io).
    BumpRecipe(BumpRecipeOpts),
}

/// Options for the debug-shell command
#[derive(Parser, Debug, Clone)]
pub struct DebugShellOpts {
    /// Work directory to use (reads from last build in rattler-build-log.txt if not specified)
    #[arg(long)]
    pub work_dir: Option<PathBuf>,

    /// Output directory containing rattler-build-log.txt
    #[arg(short, long, default_value = "./output")]
    pub output_dir: PathBuf,
}

/// Package-related subcommands.
#[derive(Parser, Debug, Clone)]
pub enum PackageCommands {
    /// Inspect and display information about a built package
    Inspect(InspectOpts),
    /// Extract a conda package to a directory
    Extract(ExtractOpts),
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

    /// The rattler-build configuration file to use
    #[arg(long, global = true)]
    pub config_file: Option<PathBuf>,

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
#[derive(Parser, Clone, Debug, Default)]
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

    /// Enable support for sharded repodata
    #[clap(long, env = "RATTLER_SHARDED", default_value = "true", hide = true)]
    pub use_sharded: bool,

    /// Enable support for JLAP (JSON Lines Append Protocol)
    #[clap(long, env = "RATTLER_JLAP", default_value = "false", hide = true)]
    pub use_jlap: bool,

    /// Enable experimental features
    #[arg(long, env = "RATTLER_BUILD_EXPERIMENTAL")]
    pub experimental: bool,

    /// List of hosts for which SSL certificate verification should be skipped
    #[arg(long, value_delimiter = ',')]
    pub allow_insecure_host: Option<Vec<String>>,

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
    #[cfg(feature = "s3")]
    pub s3_config: HashMap<String, s3_middleware::S3Config>,
    pub mirror_config: HashMap<Url, Vec<mirror_middleware::Mirror>>,
    pub allow_insecure_host: Option<Vec<String>>,
    pub use_zstd: bool,
    pub use_bz2: bool,
    pub use_sharded: bool,
    pub use_jlap: bool,
}

impl CommonData {
    /// Create a new instance of `CommonData`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output_dir: Option<PathBuf>,
        experimental: bool,
        auth_file: Option<PathBuf>,
        config: ConfigBase<()>,
        channel_priority: Option<ChannelPriority>,
        allow_insecure_host: Option<Vec<String>>,
        use_zstd: bool,
        use_bz2: bool,
        use_sharded: bool,
        use_jlap: bool,
    ) -> Self {
        // mirror config
        // todo: this is a duplicate in pixi and pixi-pack: do it like in `compute_s3_config`
        let mut mirror_config = HashMap::new();
        tracing::debug!("Using mirrors: {:?}", config.mirrors);

        fn ensure_trailing_slash(url: &url::Url) -> url::Url {
            if url.path().ends_with('/') {
                url.clone()
            } else {
                // Do not use `join` because it removes the last element
                format!("{}/", url)
                    .parse()
                    .expect("Failed to add trailing slash to URL")
            }
        }

        for (key, value) in &config.mirrors {
            let mut mirrors = Vec::new();
            for v in value {
                mirrors.push(mirror_middleware::Mirror {
                    url: ensure_trailing_slash(v),
                    no_jlap: false,
                    no_bz2: false,
                    no_zstd: false,
                    max_failures: None,
                });
            }
            mirror_config.insert(ensure_trailing_slash(key), mirrors);
        }

        #[cfg(feature = "s3")]
        let s3_config = rattler_networking::s3_middleware::compute_s3_config(&config.s3_options.0);

        Self {
            output_dir: output_dir.unwrap_or_else(|| PathBuf::from("./output")),
            experimental,
            auth_file,
            #[cfg(feature = "s3")]
            s3_config,
            mirror_config,
            channel_priority: channel_priority.unwrap_or(ChannelPriority::Strict),
            allow_insecure_host,
            use_zstd,
            use_bz2,
            use_sharded,
            use_jlap,
        }
    }

    fn from_opts_and_config(value: CommonOpts, config: ConfigBase<()>) -> Self {
        Self::new(
            value.output_dir,
            value.experimental,
            value.auth_file,
            config,
            value.channel_priority.map(|c| c.value),
            value.allow_insecure_host,
            value.use_zstd,
            value.use_bz2,
            value.use_sharded,
            value.use_jlap,
        )
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

/// Build options.
#[derive(Parser, Clone, Default)]
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
    pub channels: Option<Vec<NamedChannelOrUrl>>,

    /// Variant configuration files for the build.
    #[arg(short = 'm', long)]
    pub variant_config: Option<Vec<PathBuf>>,

    /// Override specific variant values (e.g. --variant python=3.12 or --variant python=3.12,3.11).
    /// Multiple values separated by commas will create multiple build variants.
    #[arg(long = "variant", value_parser = parse_variant_override, action = clap::ArgAction::Append)]
    pub variant_overrides: Vec<(String, Vec<String>)>,

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

    #[arg(long, env = "RATTLER_IO_CONCURRENCY_LIMIT")]
    /// The maximum number of concurrent I/O operations to use when installing packages
    /// This can be controlled by the `RATTLER_IO_CONCURRENCY_LIMIT` environment variable
    /// Defaults to 8 times the number of CPUs
    pub io_concurrency_limit: Option<usize>,

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

    /// Enable debug output in build scripts
    #[arg(long, help_heading = "Modifying result")]
    pub debug: bool,

    /// Continue building even if (one) of the packages fails to build.
    /// This is useful when building many packages with `--recipe-dir`.`
    #[clap(long)]
    pub continue_on_failure: bool,

    /// Error if the host prefix is detected in any binary files
    #[arg(long, help_heading = "Modifying result")]
    pub error_prefix_in_binary: bool,

    /// Allow symlinks in packages on Windows (defaults to false - symlinks are forbidden on Windows)
    #[arg(long, help_heading = "Modifying result")]
    pub allow_symlinks_on_windows: bool,

    /// Exclude packages newer than this date from the solver, in RFC3339 format (e.g. 2024-03-15T12:00:00Z)
    #[arg(long, help_heading = "Modifying result", value_parser = parse_datetime)]
    pub exclude_newer: Option<chrono::DateTime<chrono::Utc>>,

    /// Override the build number for all outputs (defaults to the build number in the recipe)
    #[arg(long, help_heading = "Modifying result")]
    pub build_num: Option<u64>,
}

/// Publish options for the `publish` command.
///
/// This command either builds packages from recipes OR publishes pre-built packages,
/// then uploads them to a specified channel (local or remote), followed by running indexing.
#[derive(Parser, Clone)]
pub struct PublishOpts {
    /// Package files (*.conda, *.tar.bz2) to publish directly, or recipe files (*.yaml) to build and publish.
    /// If .conda or .tar.bz2 files are provided, they will be published directly without building.
    /// If .yaml files are provided, they will be built first, then published.
    /// Use --recipe-dir (from build options below) to scan a directory for recipes instead.
    /// Defaults to "recipe.yaml" in the current directory if not specified.
    #[arg(default_value = "recipe.yaml")]
    pub package_or_recipe: Vec<PathBuf>,

    /// The channel or URL to publish the package to.
    ///
    /// Examples:
    /// - prefix.dev: https://prefix.dev/my-channel
    /// - anaconda.org: https://anaconda.org/my-org
    /// - S3: s3://my-bucket
    /// - Filesystem: file:///path/to/channel or /path/to/channel
    /// - Quetz: quetz://server.company.com/channel
    /// - Artifactory: artifactory://server.company.com/channel
    ///
    /// Note: This channel is also used as the highest priority channel when solving dependencies.
    #[arg(long = "to", help_heading = "Publishing")]
    pub to: NamedChannelOrUrl,

    /// Override the build number for all outputs.
    /// Use an absolute value (e.g., `--build-number=12`) or a relative bump (e.g., `--build-number=+1`).
    /// When using a relative bump, the highest build number from the target channel is used as the base.
    #[arg(long, help_heading = "Publishing")]
    pub build_number: Option<String>,

    /// Force upload even if the package already exists (not recommended - may break lockfiles).
    /// Only works with S3, filesystem, Anaconda.org, and prefix.dev channels.
    #[arg(long, help_heading = "Publishing")]
    pub force: bool,

    /// Automatically generate attestations when uploading to prefix.dev channels.
    /// Only works when uploading to prefix.dev channels with trusted publishing enabled.
    #[arg(long, help_heading = "Publishing")]
    pub generate_attestation: bool,

    /// Build options.
    #[clap(flatten)]
    pub build: BuildOpts,
}

#[allow(missing_docs)]
#[derive(Clone, Debug)]
pub struct PublishData {
    pub to: NamedChannelOrUrl,
    pub build_number: Option<String>,
    pub force: bool,
    pub generate_attestation: bool,
    pub package_files: Vec<PathBuf>,
    pub recipe_paths: Vec<PathBuf>,
    pub build: BuildData,
}

impl PublishData {
    /// Generate a new PublishData struct from PublishOpts and an optional config.
    pub fn from_opts_and_config(opts: PublishOpts, config: Option<ConfigBase<()>>) -> Self {
        // Separate package files from recipe paths based on file extension
        let mut package_files = Vec::new();
        let mut recipe_paths = Vec::new();

        // If recipe_dir is specified (from BuildOpts), use it; otherwise use positional arguments
        if let Some(ref recipe_dir) = opts.build.recipe_dir {
            // Use recipe_dir - will be expanded later to find all recipes in the directory
            recipe_paths.push(recipe_dir.clone());
        } else {
            // Process positional arguments
            for path in opts.package_or_recipe {
                if path.is_dir() && path.join("recipe.yaml").is_file() {
                    // If it's a directory containing recipe.yaml, treat it as a recipe path
                    recipe_paths.push(path);
                    continue;
                }

                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy();
                    if ext_str == "conda" || ext_str == "bz2" {
                        package_files.push(path);
                        continue;
                    } else if ext_str == "yaml" || ext_str == "yml" {
                        recipe_paths.push(path);
                        continue;
                    }
                }
            }
        }

        // Prepend the --to channel to the list of channels for dependency resolution
        let mut build_opts = opts.build;
        let to_channel = opts.to.clone();

        // Add the to channel as the first channel (highest priority)
        let channels = if let Some(mut channels) = build_opts.channels.take() {
            channels.insert(0, to_channel.clone());
            Some(channels)
        } else {
            Some(vec![to_channel.clone()])
        };

        build_opts.channels = channels;

        Self {
            to: opts.to,
            build_number: opts.build_number,
            force: opts.force,
            generate_attestation: opts.generate_attestation,
            package_files,
            recipe_paths,
            build: BuildData::from_opts_and_config(build_opts, config),
        }
    }
}

#[allow(missing_docs)]
#[derive(Clone, Debug)]
pub struct BuildData {
    pub up_to: Option<String>,
    pub build_platform: Platform,
    pub target_platform: Platform,
    pub host_platform: Platform,
    pub channels: Option<Vec<NamedChannelOrUrl>>,
    pub variant_config: Vec<PathBuf>,
    pub variant_overrides: HashMap<String, Vec<String>>,
    pub ignore_recipe_variants: bool,
    pub render_only: bool,
    pub with_solve: bool,
    pub keep_build: bool,
    pub no_build_id: bool,
    pub package_format: PackageFormatAndCompression,
    pub compression_threads: Option<u32>,
    pub io_concurrency_limit: usize,
    pub no_include_recipe: bool,
    pub test: TestStrategy,
    pub color_build_log: bool,
    pub common: CommonData,
    pub tui: bool,
    pub skip_existing: SkipExisting,
    pub noarch_build_platform: Option<Platform>,
    pub extra_meta: Option<Vec<(String, Value)>>,
    pub sandbox_configuration: Option<SandboxConfiguration>,
    pub debug: Debug,
    pub continue_on_failure: ContinueOnFailure,
    pub error_prefix_in_binary: bool,
    pub allow_symlinks_on_windows: bool,
    pub exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    pub build_num_override: Option<u64>,
}

impl BuildData {
    /// Creates a new instance of `BuildData`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        up_to: Option<String>,
        build_platform: Option<Platform>,
        target_platform: Option<Platform>,
        host_platform: Option<Platform>,
        channels: Option<Vec<NamedChannelOrUrl>>,
        variant_config: Option<Vec<PathBuf>>,
        variant_overrides: HashMap<String, Vec<String>>,
        ignore_recipe_variants: bool,
        render_only: bool,
        with_solve: bool,
        keep_build: bool,
        no_build_id: bool,
        package_format: Option<PackageFormatAndCompression>,
        compression_threads: Option<u32>,
        io_concurrency_limit: Option<usize>,
        no_include_recipe: bool,
        test: Option<TestStrategy>,
        common: CommonData,
        tui: bool,
        skip_existing: Option<SkipExisting>,
        noarch_build_platform: Option<Platform>,
        extra_meta: Option<Vec<(String, Value)>>,
        sandbox_configuration: Option<SandboxConfiguration>,
        debug: Debug,
        continue_on_failure: ContinueOnFailure,
        error_prefix_in_binary: bool,
        allow_symlinks_on_windows: bool,
        exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
        build_num_override: Option<u64>,
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
            variant_overrides,
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
            io_concurrency_limit: io_concurrency_limit.unwrap_or(num_cpus::get() * 8),
            no_include_recipe,
            test: test.unwrap_or_default(),
            color_build_log: true,
            common,
            tui,
            skip_existing: skip_existing.unwrap_or(SkipExisting::None),
            noarch_build_platform,
            extra_meta,
            sandbox_configuration,
            debug,
            continue_on_failure,
            error_prefix_in_binary,
            allow_symlinks_on_windows,
            exclude_newer,
            build_num_override,
        }
    }
}

impl BuildData {
    /// Generate a new BuildData struct from BuildOpts and an optional pixi config.
    /// BuildOpts have higher priority than the pixi config.
    pub fn from_opts_and_config(opts: BuildOpts, config: Option<ConfigBase<()>>) -> Self {
        Self::new(
            opts.up_to,
            opts.build_platform,
            opts.target_platform, // todo: read this from config as well
            opts.host_platform,
            opts.channels.or_else(|| {
                config
                    .as_ref()
                    .and_then(|config| config.default_channels.clone())
            }),
            opts.variant_config,
            opts.variant_overrides.into_iter().collect(),
            opts.ignore_recipe_variants,
            opts.render_only,
            opts.with_solve,
            opts.keep_build,
            opts.no_build_id,
            opts.package_format.or_else(|| {
                config
                    .as_ref()
                    .and_then(|config| config.build.package_format.clone())
            }),
            opts.compression_threads,
            opts.io_concurrency_limit,
            opts.no_include_recipe,
            opts.test.or(if opts.no_test {
                Some(TestStrategy::Skip)
            } else {
                None
            }),
            CommonData::from_opts_and_config(opts.common, config.unwrap_or_default()),
            opts.tui,
            opts.skip_existing,
            opts.noarch_build_platform,
            opts.extra_meta,
            opts.sandbox_arguments.into(),
            Debug::new(opts.debug),
            opts.continue_on_failure.into(),
            opts.error_prefix_in_binary,
            opts.allow_symlinks_on_windows,
            opts.exclude_newer,
            opts.build_num,
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

/// Parse variant override (e.g., "python=3.12" or "python=3.12,3.11")
fn parse_variant_override(
    s: &str,
) -> Result<(String, Vec<String>), Box<dyn Error + Send + Sync + 'static>> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{}`", s))?;

    let values: Vec<String> = value.split(',').map(|v| v.trim().to_string()).collect();
    Ok((key.to_string(), values))
}

/// Parse a datetime string in RFC3339 format
fn parse_datetime(s: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| {
            format!(
                "Invalid datetime format '{}': {}. Expected RFC3339 format (e.g., 2024-03-15T12:00:00Z)",
                s, e
            )
        })
}

/// Test options.
#[derive(Parser)]
pub struct TestOpts {
    /// Channels to use when testing
    #[arg(short = 'c', long = "channel")]
    pub channels: Option<Vec<NamedChannelOrUrl>>,

    /// The package file to test
    #[arg(short, long)]
    pub package_file: PathBuf,

    /// The number of threads to use for compression.
    #[clap(long, env = "RATTLER_COMPRESSION_THREADS")]
    pub compression_threads: Option<u32>,

    /// The index of the test to run. This is used to run a specific test from the package.
    #[clap(long)]
    pub test_index: Option<usize>,

    /// Build test environment and output debug information for manual debugging.
    #[arg(long)]
    pub debug: bool,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct TestData {
    pub channels: Option<Vec<NamedChannelOrUrl>>,
    pub package_file: PathBuf,
    pub compression_threads: Option<u32>,
    pub common: CommonData,
    pub test_index: Option<usize>,
    pub debug: Debug,
}

impl TestData {
    /// Generate a new TestData struct from TestOpts and an optional pixi config.
    /// TestOpts have higher priority than the pixi config.
    pub fn from_opts_and_config(value: TestOpts, config: Option<ConfigBase<()>>) -> Self {
        Self::new(
            value.package_file,
            value.channels,
            value.compression_threads,
            Debug::new(value.debug),
            value.test_index,
            CommonData::from_opts_and_config(value.common, config.unwrap_or_default()),
        )
    }

    /// Create a new instance of `TestData`
    pub fn new(
        package_file: PathBuf,
        channels: Option<Vec<NamedChannelOrUrl>>,
        compression_threads: Option<u32>,
        debug: Debug,
        test_index: Option<usize>,
        common: CommonData,
    ) -> Self {
        Self {
            package_file,
            channels,
            compression_threads,
            test_index,
            debug,
            common,
        }
    }
}

/// Represents a package source that can be either a local path or a URL
#[derive(Debug, Clone)]
pub enum PackageSource {
    /// Local file path
    Path(PathBuf),
    /// Remote URL
    Url(Url),
}

impl FromStr for PackageSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try to parse as URL first
        if s.starts_with("http://") || s.starts_with("https://") {
            match Url::parse(s) {
                Ok(url) => Ok(PackageSource::Url(url)),
                Err(e) => Err(format!("Invalid URL: {}", e)),
            }
        } else {
            // Treat as local path
            Ok(PackageSource::Path(PathBuf::from(s)))
        }
    }
}

/// Rebuild options.
#[derive(Parser)]
pub struct RebuildOpts {
    /// The package file to rebuild (can be a local path or URL)
    #[arg(short, long)]
    pub package_file: PackageSource,

    /// Do not run tests after building (deprecated, use `--test=skip` instead)
    #[arg(long, hide = true)]
    pub no_test: bool,

    /// The strategy to use for running tests
    #[arg(long, help_heading = "Modifying result")]
    pub test: Option<TestStrategy>,

    /// The number of threads to use for compression.
    #[clap(long, env = "RATTLER_COMPRESSION_THREADS")]
    pub compression_threads: Option<u32>,

    /// The number of threads to use for I/O operations when installing packages.
    #[clap(long, env = "RATTLER_IO_CONCURRENCY_LIMIT")]
    pub io_concurrency_limit: Option<usize>,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

#[derive(Debug)]
#[allow(missing_docs)]
pub struct RebuildData {
    pub package_file: PackageSource,
    pub test: TestStrategy,
    pub compression_threads: Option<u32>,
    pub common: CommonData,
}

impl RebuildData {
    /// Generate a new RebuildData struct from RebuildOpts and an optional pixi config.
    /// RebuildOpts have higher priority than the pixi config.
    pub fn from_opts_and_config(value: RebuildOpts, config: Option<ConfigBase<()>>) -> Self {
        Self::new(
            value.package_file,
            value.test.unwrap_or(if value.no_test {
                TestStrategy::Skip
            } else {
                TestStrategy::default()
            }),
            value.compression_threads,
            CommonData::from_opts_and_config(value.common, config.unwrap_or_default()),
        )
    }

    /// Create a new instance of `RebuildData`
    pub fn new(
        package_file: PackageSource,
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

/// Debug options
#[derive(Parser)]
pub struct DebugOpts {
    /// Recipe file to debug
    #[arg(short, long)]
    pub recipe: PathBuf,

    /// Output directory for build artifacts
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// The target platform to build for
    #[arg(long)]
    pub target_platform: Option<Platform>,

    /// The host platform to build for (defaults to target_platform)
    #[arg(long)]
    pub host_platform: Option<Platform>,

    /// The build platform to build for (defaults to current platform)
    #[arg(long)]
    pub build_platform: Option<Platform>,

    /// Channels to use when building
    #[arg(short = 'c', long = "channel")]
    pub channels: Option<Vec<NamedChannelOrUrl>>,

    /// Common options
    #[clap(flatten)]
    pub common: CommonOpts,

    /// Name of the specific output to debug (only required when a recipe has multiple outputs)
    #[arg(long, help = "Name of the specific output to debug")]
    pub output_name: Option<String>,
}

#[derive(Debug, Clone)]
/// Data structure containing the configuration for debugging a recipe
pub struct DebugData {
    /// Path to the recipe file to debug
    pub recipe_path: PathBuf,
    /// Directory where build artifacts will be stored
    pub output_dir: PathBuf,
    /// Platform where the build is being executed
    pub build_platform: Platform,
    /// Target platform for the build
    pub target_platform: Platform,
    /// Host platform for runtime dependencies
    pub host_platform: Platform,
    /// List of channels to search for dependencies
    pub channels: Option<Vec<NamedChannelOrUrl>>,
    /// Common configuration options
    pub common: CommonData,
    /// Name of the specific output to debug (if recipe has multiple outputs)
    pub output_name: Option<String>,
}

impl DebugData {
    /// Generate a new TestData struct from TestOpts and an optional pixi config.
    /// TestOpts have higher priority than the pixi config.
    pub fn from_opts_and_config(opts: DebugOpts, config: Option<ConfigBase<()>>) -> Self {
        Self {
            recipe_path: opts.recipe,
            output_dir: opts.output.unwrap_or_else(|| PathBuf::from("./output")),
            build_platform: opts.build_platform.unwrap_or(Platform::current()),
            target_platform: opts.target_platform.unwrap_or(Platform::current()),
            host_platform: opts
                .host_platform
                .unwrap_or_else(|| opts.target_platform.unwrap_or(Platform::current())),
            channels: opts.channels,
            common: CommonData::from_opts_and_config(opts.common, config.unwrap_or_default()),
            output_name: opts.output_name,
        }
    }
}

/// Options for the `create-patch` command.
#[derive(Parser, Debug, Clone)]
pub struct CreatePatchOpts {
    /// Directory where we want to create the patch.
    /// Defaults to current directory if not specified.
    #[arg(short, long)]
    pub directory: Option<PathBuf>,

    /// The name for the patch file to create.
    #[arg(long, default_value = "changes")]
    pub name: String,

    /// Whether to overwrite the patch file if it already exists.
    #[arg(long, default_value = "false")]
    pub overwrite: bool,

    /// Optional directory where the patch file should be written. Defaults to the recipe directory determined from `.source_info.json` if not provided.
    #[arg(long, value_name = "DIR")]
    pub patch_dir: Option<PathBuf>,

    /// Comma-separated list of file names (or glob patterns) that should be excluded from the diff.
    #[arg(long, value_delimiter = ',')]
    pub exclude: Option<Vec<String>>,

    /// Include new files matching these glob patterns (e.g., "*.txt", "src/**/*.rs")
    #[arg(long, value_delimiter = ',')]
    pub add: Option<Vec<String>>,

    /// Only include modified files matching these glob patterns (e.g., "*.c", "src/**/*.rs")
    /// If not specified, all modified files are included (subject to --exclude)
    #[arg(long, value_delimiter = ',')]
    pub include: Option<Vec<String>>,

    /// Perform a dry-run: analyze changes and log the diff, but don't write the patch file.
    #[arg(long, default_value = "false")]
    pub dry_run: bool,
}

/// Options for the `package inspect` command.
#[derive(Parser, Debug, Clone)]
pub struct InspectOpts {
    /// Path to the package file (.conda, .tar.bz2)
    pub package_file: PathBuf,

    /// Show detailed file listing with hashes and sizes
    #[arg(long)]
    pub paths: bool,

    /// Show extended about information
    #[arg(long)]
    pub about: bool,

    /// Show run exports
    #[arg(long)]
    pub run_exports: bool,

    /// Show all available information
    #[arg(long)]
    pub all: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

impl InspectOpts {
    /// Check if paths should be shown (either explicitly or via --all)
    pub fn show_paths(&self) -> bool {
        self.paths || self.all
    }

    /// Check if about info should be shown (either explicitly or via --all)
    pub fn show_about(&self) -> bool {
        self.about || self.all
    }

    /// Check if run exports should be shown (either explicitly or via --all)
    pub fn show_run_exports(&self) -> bool {
        self.run_exports || self.all
    }
}

/// Options for the `package extract` command.
#[derive(Parser, Debug, Clone)]
pub struct ExtractOpts {
    /// Path to the package file (.conda, .tar.bz2) or a URL to download from
    pub package_file: PackageSource,

    /// Destination directory for extraction (defaults to package name without extension)
    #[arg(short = 'd', long)]
    pub dest: Option<PathBuf>,
}

/// Options for the `bump-recipe` command.
#[derive(Parser, Debug, Clone)]
pub struct BumpRecipeOpts {
    /// Path to the recipe file (recipe.yaml). Defaults to current directory.
    #[arg(short, long, default_value = ".")]
    pub recipe: PathBuf,

    /// The new version to bump to. If not specified, will auto-detect the latest
    /// version from the source URL's provider (GitHub, PyPI, crates.io).
    #[arg(long)]
    pub version: Option<String>,

    /// Include pre-release versions when auto-detecting (e.g., alpha, beta, rc).
    #[arg(long, default_value = "false")]
    pub include_prerelease: bool,

    /// Only check for updates without modifying the recipe.
    #[arg(long, default_value = "false")]
    pub check_only: bool,

    /// Perform a dry-run: show what would be changed without writing to the file.
    #[arg(long, default_value = "false")]
    pub dry_run: bool,

    /// Keep the current build number instead of resetting it to 0.
    #[arg(long, default_value = "false")]
    pub keep_build_number: bool,
}
