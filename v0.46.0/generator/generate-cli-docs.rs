/// This is a separate binary not included in the main rattler-build binary.
/// Used to generate the documentation for the rattler-build binary.

#[cfg(feature = "generate-cli-docs")]
fn main() {
    use clap_markdown::help_markdown;
    use rattler_build::opt::App;
    use rattler_conda_types::Platform;

    let help = help_markdown::<App>();

    let target_default_platform = format!("Default value: `{}`", Platform::current());
    let help = help.replace(
        target_default_platform.as_str(),
        "Default value: current platform",
    );

    print!("{}", help);
}

#[cfg(not(feature = "generate-cli-docs"))]
fn main() {
    eprintln!("This binary is not enabled in the current build configuration.");
    eprintln!("To enable it, run `cargo build --features generate-cli-docs`.");
    std::process::exit(1);
}
