//! Script execution and sandbox configuration for Rattler-Build, supporting bash, cmd,
//! python, and other interpreters.
//!
//! This crate provides functionality for defining, parsing, and executing build scripts
//! in various interpreters as part of the Rattler-Build process.
//!
//! Execution model: every script runs through a platform-native wrapper (bash on
//! Unix, cmd on Windows) that first performs prefix activation, then invokes the
//! chosen interpreter. Inline or file-backed scripts for specialized interpreters
//! (python, perl, etc.) are written out and executed by the activated wrapper.

pub mod sandbox;
mod script;

pub use sandbox::{
    PlatformKind, PlatformOverride, SandboxArguments, SandboxConfigFile, SandboxConfiguration,
    SandboxRequest,
};
pub use script::{
    Script, ScriptContent, determine_interpreter_from_path, platform_script_extensions,
};

#[cfg(feature = "execution")]
mod activation;
#[cfg(feature = "execution")]
mod execution;
#[cfg(feature = "execution")]
mod execution_context;
#[cfg(feature = "execution")]
mod interpreter;
#[cfg(feature = "execution")]
pub mod runner;
#[cfg(feature = "execution")]
mod runtime;
#[cfg(feature = "execution")]
mod shell_dialect;
#[cfg(feature = "execution")]
mod windows_machine;

#[cfg(feature = "execution")]
pub use execution::{
    BuildScriptSection, EnvironmentIsolation, ExecutionArgs, ResolvedScriptContents,
    create_build_script, run_script,
};
#[cfg(feature = "execution")]
pub use execution_context::{ExecutionContext, PrefixLayout, PrefixWithPlatform};
#[cfg(feature = "execution")]
pub use interpreter::{InterpreterError, closest_interpreter};
#[cfg(feature = "execution")]
pub use runtime::RuntimeEnv;
