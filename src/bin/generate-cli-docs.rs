/// This is a separate binary not included in the main rattler-build binary.
/// Used to generate the documentation for the rattler-build binary.

#[cfg(feature = "generate-cli-docs")]
fn main() {
    use clap_markdown::print_help_markdown;
    use rattler_build::opt::App;

    print_help_markdown::<App>();
}

#[cfg(not(feature = "generate-cli-docs"))]
fn main() {
    eprintln!("This binary is not enabled in the current build configuration.");
    eprintln!("To enable it, run `cargo build --features generate-cli-docs`.");
    std::process::exit(1);
}
