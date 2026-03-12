//! Script execution and sandbox configuration for Rattler-Build, supporting bash, cmd,
//! python, and other interpreters.
//!
//! This crate provides functionality for defining, parsing, and executing build scripts
//! in various interpreters as part of the Rattler-Build process.

pub mod sandbox;
mod script;

pub use sandbox::{SandboxArguments, SandboxConfiguration};
pub use script::{Script, ScriptContent, determine_interpreter_from_path};

#[cfg(feature = "execution")]
mod execution;
#[cfg(feature = "execution")]
mod interpreter;

#[cfg(feature = "execution")]
pub use execution::{
    Debug, ExecutionArgs, ResolvedScriptContents, create_build_script,
    run_process_with_replacements, run_script,
};
#[cfg(feature = "execution")]
pub use interpreter::InterpreterError;
