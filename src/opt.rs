//! Command-line options.

use std::{path::PathBuf, str::FromStr};

use crate::{
    console_utils::{Color, LogStyle},
    recipe_generator::GenerateRecipeOpts,
};
use clap::builder::ArgPredicate;
use clap::{arg, crate_version, Parser};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use rattler_conda_types::{package::ArchiveType, Platform};
use rattler_package_streaming::write::CompressionLevel;
use url::Url;

/// Application subcommands.
#[derive(Parser)]
pub enum SubCommands {
    /// Build a package
    Build(BuildOpts),

    /// Test a package
    Test(TestOpts),

    /// Rebuild a package
    Rebuild(RebuildOpts),

    /// Upload a package
    Upload(UploadOpts),

    /// Generate shell completion script
    Completion(ShellCompletion),

    /// Generate a recipe from PyPI or CRAN
    GenerateRecipe(GenerateRecipeOpts),

    /// Handle authentication to external repositories
    Auth(rattler::cli::auth::Args),
}

/// Shell completion options.
#[derive(Parser)]
pub struct ShellCompletion {
    /// Shell.
    #[arg(short, long)]
    pub shell: Option<clap_complete::Shell>,
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
#[derive(Parser, Clone)]
pub struct CommonOpts {
    /// Output directory for build artifacts. Defaults to `./output`.
    #[clap(long, env = "CONDA_BLD_PATH")]
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
    /// The recipe file or directory containing `recipe.yaml`. Defaults to the current directory.
    #[arg(
        short,
        long,
        default_value = ".",
        default_value_if("recipe_dir", ArgPredicate::IsPresent, None)
    )]
    pub recipe: Vec<PathBuf>,

    /// The directory that contains recipes.
    #[arg(long)]
    pub recipe_dir: Option<PathBuf>,

    /// Build recipes up to the specified package.
    #[arg(long)]
    pub up_to: Option<String>,

    /// The build platform to use for the build (e.g. for building with emulation, or rendering).
    #[arg(long, default_value_t = Platform::current())]
    pub build_platform: Platform,

    /// The target platform for the build.
    #[arg(long, default_value_t = Platform::current())]
    pub target_platform: Platform,

    /// Add the channels needed for the recipe using this option. For more then one channel use it multiple times.
    /// The default channel is `conda-forge`.
    #[arg(short = 'c', long)]
    pub channel: Option<Vec<String>>,

    /// Variant configuration files for the build.
    #[arg(short = 'm', long)]
    pub variant_config: Vec<PathBuf>,

    /// Render the recipe files without executing the build.
    #[arg(long)]
    pub render_only: bool,

    /// Solve the dependencies of recipe
    #[arg(long, requires("render_only"))]
    pub no_solve: bool,

    /// Keep intermediate build artifacts after the build.
    #[arg(long)]
    pub keep_build: bool,

    /// Don't use build id(timestamp) when creating build directory name. Defaults to `false`.
    #[arg(long)]
    pub no_build_id: bool,

    /// The package format to use for the build. Can be one of `tar-bz2` or `conda`.
    /// You can also add a compression level to the package format, e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to 22).
    #[arg(long, default_value = "conda")]
    pub package_format: PackageFormatAndCompression,

    #[arg(long)]
    /// The number of threads to use for compression (only relevant when also using `--package-format conda`)
    pub compression_threads: Option<u32>,

    /// Do not store the recipe in the final package
    #[arg(long)]
    pub no_include_recipe: bool,

    /// Do not run tests after building
    #[arg(long, default_value = "false")]
    pub no_test: bool,

    /// Do not force colors in the output of the build script
    #[arg(long, default_value = "true")]
    pub color_build_log: bool,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,

    /// Launch the terminal user interface.
    #[arg(long, default_value = "false", hide = !cfg!(feature = "tui"))]
    pub tui: bool,
}

/// Test options.
#[derive(Parser)]
pub struct TestOpts {
    /// Channels to use when testing
    #[arg(short = 'c', long)]
    pub channel: Option<Vec<String>>,

    /// The package file to test
    #[arg(short, long)]
    pub package_file: PathBuf,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

/// Rebuild options.
#[derive(Parser)]
pub struct RebuildOpts {
    /// The package file to rebuild
    #[arg(short, long)]
    pub package_file: PathBuf,

    /// Do not run tests after building
    #[arg(long, default_value = "false")]
    pub no_test: bool,

    /// Common options.
    #[clap(flatten)]
    pub common: CommonOpts,
}

/// Upload options.
#[derive(Parser)]
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

#[derive(Clone, Debug, PartialEq, Parser)]
/// Options for uploading to a Quetz server.
/// Authentication is used from the keychain / auth-file.
pub struct QuetzOpts {
    /// The URL to your Quetz server
    #[arg(short, long, env = "QUETZ_SERVER_URL")]
    pub url: Url,

    /// The URL to your channel
    #[arg(short, long, env = "QUETZ_CHANNEL")]
    pub channel: String,

    /// The Quetz API key, if none is provided, the token is read from the keychain / auth-file
    #[arg(short, long, env = "QUETZ_API_KEY")]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Parser)]
/// Options for uploading to a Artifactory channel.
/// Authentication is used from the keychain / auth-file.
pub struct ArtifactoryOpts {
    /// The URL to your Artifactory server
    #[arg(short, long, env = "ARTIFACTORY_SERVER_URL")]
    pub url: Url,

    /// The URL to your channel
    #[arg(short, long, env = "ARTIFACTORY_CHANNEL")]
    pub channel: String,

    /// Your Artifactory username
    #[arg(short = 'r', long, env = "ARTIFACTORY_USERNAME")]
    pub username: Option<String>,

    /// Your Artifactory password
    #[arg(short, long, env = "ARTIFACTORY_PASSWORD")]
    pub password: Option<String>,
}

/// Options for uploading to a prefix.dev server.
/// Authentication is used from the keychain / auth-file
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct PrefixOpts {
    /// The URL to the prefix.dev server (only necessary for self-hosted instances)
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

    /// The prefix.dev API key, if none is provided, the token is read from the keychain / auth-file
    #[arg(short, long, env = "PREFIX_API_KEY")]
    pub api_key: Option<String>,
}

/// Options for uploading to a Anaconda.org server
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct AnacondaOpts {
    /// The owner of the distribution (e.g. conda-forge or your username)
    #[arg(short, long, env = "ANACONDA_OWNER")]
    pub owner: String,

    /// The channel / label to upload the package to (e.g. main / rc)
    #[arg(short, long, env = "ANACONDA_CHANNEL", default_value = "main", num_args = 1..)]
    pub channel: Vec<String>,

    /// The Anaconda API key, if none is provided, the token is read from the keychain / auth-file
    #[arg(short, long, env = "ANACONDA_API_KEY")]
    pub api_key: Option<String>,

    /// The URL to the Anaconda server
    #[arg(
        short,
        long,
        env = "ANACONDA_SERVER_URL",
        default_value = "https://api.anaconda.org"
    )]
    pub url: Url,

    /// Replace files on conflict
    #[arg(long, short, env = "ANACONDA_FORCE", default_value = "false")]
    pub force: bool,
}

/// Options for uploading to conda-forge
#[derive(Clone, Debug, PartialEq, Parser)]
pub struct CondaForgeOpts {
    /// The Anaconda API key
    #[arg(long, env = "STAGING_BINSTAR_TOKEN", required = true)]
    pub staging_token: String,

    /// The feedstock name
    #[arg(long, env = "FEEDSTOCK_NAME", required = true)]
    pub feedstock: String,

    /// The feedstock token
    #[arg(long, env = "FEEDSTOCK_TOKEN", required = true)]
    pub feedstock_token: String,

    /// The staging channel name
    #[arg(long, env = "STAGING_CHANNEL", default_value = "cf-staging")]
    pub staging_channel: String,

    /// The Anaconda Server URL
    #[arg(
        long,
        env = "ANACONDA_SERVER_URL",
        default_value = "https://api.anaconda.org"
    )]
    pub anaconda_url: Url,

    /// The validation endpoint url
    #[arg(
        long,
        env = "VALIDATION_ENDPOINT",
        default_value = "https://conda-forge.herokuapp.com/feedstock-outputs/copy"
    )]
    pub validation_endpoint: Url,

    /// Post comment on promotion failure
    #[arg(long, env = "POST_COMMENT_ON_ERROR", default_value = "true")]
    pub post_comment_on_error: bool,

    /// The CI provider
    #[arg(long, env = "CI")]
    pub provider: Option<String>,

    /// Dry run, don't actually upload anything
    #[arg(long, env = "DRY_RUN", default_value = "false")]
    pub dry_run: bool,
}

#[cfg(test)]
mod test {
    use super::PackageFormatAndCompression;
    use rattler_conda_types::package::ArchiveType;
    use rattler_package_streaming::write::CompressionLevel;
    use std::str::FromStr;

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
