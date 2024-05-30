/// This is a separate binary not included in the main rattler-build binary.
/// Used to generate the documentation for the rattler-build binary.

#[cfg(feature = "generate-cli-docs")]
use clap_markdown::print_help_markdown;
use rattler_build::opt::App;

fn main() {
    print_help_markdown::<App>();
}

