#![deny(missing_docs)]

//! The library pieces of rattler-build

pub mod build;
pub mod metadata;
pub mod package_test;
pub mod packaging;
pub mod recipe;
pub mod render;
pub mod selectors;
pub mod source;
pub mod system_tools;
pub mod tool_configuration;
pub mod used_variables;
pub mod utils;
pub mod variant_config;

mod env_vars;
pub mod hash;
mod linux;
mod macos;
mod post_process;
pub mod recipe_generator;
mod unix;
mod windows;
