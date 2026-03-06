#![deny(missing_docs)]

//! Core library for rattler-build.

pub mod build;
pub mod bump_recipe;
pub mod console_utils;
pub mod metadata;
pub mod migrate_recipe;
pub mod package_test;
pub mod packaging;
pub mod render;
pub mod script;
pub mod source;
pub mod staging;
pub mod system_tools;
pub mod tool_configuration;

pub mod types;
pub mod utils;

/// Constants used throughout the build process.
pub mod consts;
pub mod env_vars;
mod linux;
mod macos;
pub mod package_info;
mod post_process;
pub mod publish;
pub mod rebuild;
mod unix;
mod windows;

mod package_cache_reporter;

// Re-export types needed by Python bindings and external consumers
pub use rattler_build_jinja::Variable;
pub use rattler_build_recipe::stage1::Recipe;
pub use rattler_build_recipe::stage1::build::BuildString;
pub use rattler_build_recipe::variant_render::RenderConfig;

use std::collections::BTreeMap;

use rattler_build_recipe::stage1::HashInfo;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{NoArchType, Platform};

/// A discovered output from variant expansion
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct DiscoveredOutput {
    pub name: String,
    pub version: String,
    pub build_string: String,
    pub noarch_type: NoArchType,
    pub target_platform: Platform,
    pub used_vars: BTreeMap<NormalizedKey, Variable>,
    pub recipe: Recipe,
    pub hash: HashInfo,
}

impl Eq for DiscoveredOutput {}

impl PartialEq for DiscoveredOutput {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.build_string == other.build_string
            && self.noarch_type == other.noarch_type
            && self.target_platform == other.target_platform
    }
}

impl std::hash::Hash for DiscoveredOutput {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.version.hash(state);
        self.build_string.hash(state);
        self.noarch_type.hash(state);
        self.target_platform.hash(state);
    }
}
