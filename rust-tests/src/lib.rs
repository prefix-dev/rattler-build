#[cfg(test)]
mod tests {
    #[test]
    fn rattler_test_help() {
        let help_test = std::process::Command::new("cargo")
            .arg("run")
            .args(["-q", "-p", "rattler-build"])
            .output()
            .map(|out| out.stderr)
            .ok()
            .map(|bytes| String::from_utf8(bytes).ok())
            .flatten()
            .map(|s| s.starts_with("Usage: rattler-build [OPTIONS]"))
            .unwrap_or_default();
        assert!(help_test);
    }
}
