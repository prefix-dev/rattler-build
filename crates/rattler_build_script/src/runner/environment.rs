use indexmap::IndexMap;
use rattler_conda_types::Platform;

use crate::{EnvironmentIsolation, RuntimeEnv};

/// Environment variables passed through from the host environment because they
/// cannot be computed by rattler-build.
const PASSTHROUGH_ENV_VARS: &[&str] = &[
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "SSH_AUTH_SOCK",
    "DISPLAY",
    "http_proxy",
    "https_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "no_proxy",
    "NO_PROXY",
];

/// Platform-critical environment variables required for basic OS functionality.
fn platform_passthrough_vars(platform: Platform) -> &'static [&'static str] {
    if platform.is_windows() {
        &["SYSTEMROOT", "WINDIR", "COMSPEC", "TEMP", "TMP", "PATHEXT"]
    } else if platform.is_osx() {
        &["TMPDIR", "__CF_USER_TEXT_ENCODING"]
    } else {
        &[]
    }
}

/// Computes the complete child environment for the given isolation mode.
///
/// This reads only its arguments and never the ambient process environment.
pub(crate) fn resolve_process_env(
    env_isolation: EnvironmentIsolation,
    env_vars: &IndexMap<String, String>,
    secrets: &IndexMap<String, String>,
    runtime: &RuntimeEnv,
) -> IndexMap<String, String> {
    match env_isolation {
        EnvironmentIsolation::Strict | EnvironmentIsolation::CondaBuild => {
            let mut process_env = IndexMap::new();

            for var in PASSTHROUGH_ENV_VARS
                .iter()
                .chain(platform_passthrough_vars(runtime.process_platform()))
            {
                if let Some(value) = runtime.var(var) {
                    process_env.insert((*var).to_owned(), value.to_owned());
                }
            }

            process_env.extend(env_vars.clone());
            process_env.extend(secrets.clone());
            process_env
        }
        EnvironmentIsolation::None => {
            let mut process_env = runtime
                .vars()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect::<IndexMap<_, _>>();
            process_env.extend(env_vars.clone());
            process_env
        }
    }
}
