use std::{process::Command, sync::Arc};

use minijinja::{
    Value,
    value::{Object, from_args},
};

#[derive(Debug)]
pub(crate) struct Git {
    pub(crate) experimental: bool,
}

impl std::fmt::Display for Git {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Git")
    }
}

fn get_command_output(command: &str, args: &[&str]) -> Result<String, minijinja::Error> {
    let output = Command::new(command).args(args).output().map_err(|e| {
        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
    })?;

    if !output.status.success() {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    } else {
        Ok(String::from_utf8(output.stdout).map_err(|e| {
            minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
        })?)
    }
}

impl Git {
    fn head_rev(&self, src: &str) -> Result<Value, minijinja::Error> {
        let result = get_command_output("git", &["ls-remote", src, "HEAD"])?
            .lines()
            .next()
            .and_then(|s| s.split_ascii_whitespace().nth(0))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "Failed to get the HEAD".to_string(),
                )
            })?
            .to_string();
        Ok(Value::from(result))
    }

    fn latest_tag_rev(&self, src: &str) -> Result<Value, minijinja::Error> {
        let result = get_command_output("git", &["ls-remote", "--tags", "--sort=-v:refname", src])?
            .lines()
            .next()
            .and_then(|s| s.split_ascii_whitespace().nth(0))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "Failed to get the latest tag".to_string(),
                )
            })?
            .to_string();
        Ok(Value::from(result))
    }

    fn latest_tag(&self, src: &str) -> Result<Value, minijinja::Error> {
        let result = get_command_output("git", &["ls-remote", "--tags", "--sort=-v:refname", src])?
            .lines()
            .next()
            .and_then(|s| s.split_ascii_whitespace().nth(1))
            .and_then(|s| s.strip_prefix("refs/tags/"))
            .map(|s| s.trim_end_matches("^{}"))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "Failed to get the latest tag".to_string(),
                )
            })?
            .to_string();
        Ok(Value::from(result))
    }
}

impl Object for Git {
    fn call_method(
        self: &Arc<Self>,
        _state: &minijinja::State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        if !self.experimental {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "Experimental feature: provide the `--experimental` flag to enable this feature",
            ));
        }
        let (src,) = from_args(args)?;
        match name {
            "head_rev" => self.head_rev(src),
            "latest_tag_rev" => self.latest_tag_rev(src),
            "latest_tag" => self.latest_tag(src),
            name => Err(minijinja::Error::new(
                minijinja::ErrorKind::UnknownMethod,
                format!("object has no method named {name}"),
            )),
        }
    }
}
