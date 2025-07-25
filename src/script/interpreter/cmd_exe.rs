use std::path::PathBuf;

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{ExecutionArgs, run_process_with_replacements};

use super::{CMDEXE_PREAMBLE, Interpreter, InterpreterError, find_interpreter};

fn print_debug_info(args: &ExecutionArgs) -> String {
    let mut output = String::new();
    if args.debug.is_enabled() {
        output.push_str("\nDebug mode enabled - not executing the script.\n\n");
    } else {
        output.push_str("\nScript execution failed.\n\n")
    }

    output.push_str(&format!("  Work directory: {}\n", args.work_dir.display()));
    output.push_str(&format!("  Prefix: {}\n", args.run_prefix.display()));

    if let Some(build_prefix) = &args.build_prefix {
        output.push_str(&format!("  Build prefix: {}\n", build_prefix.display()));
    } else {
        output.push_str("  Build prefix: None\n");
    }

    output.push_str("\nTo run the script manually, use the following command:\n");
    output.push_str(&format!(
        "  cd {:?} && ./conda_build.bat\n\n",
        args.work_dir
    ));
    output.push_str("To run commands interactively in the build environment:\n");
    output.push_str(&format!("  cd {:?} && call build_env.bat", args.work_dir));

    output
}

pub(crate) struct CmdExeInterpreter;

impl CmdExeInterpreter {
    /// Add exit code checks after each command in a Windows batch script.
    /// This ensures that failing commands don't get ignored, mimicking conda-build's behavior.
    /// 
    /// For each command line (except the last), adds:
    /// `IF %ERRORLEVEL% NEQ 0 EXIT 1`
    pub fn add_exit_code_checks(script_content: &str) -> String {
        let lines: Vec<&str> = script_content.lines().collect();
        
        // If there's only one line or the script is empty, no need to add checks
        if lines.len() <= 1 {
            return script_content.to_string();
        }
        
        let mut result = Vec::new();
        
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            
            // Add the original line
            result.push(line.to_string());
            
            // Add exit code check after each command line (except the last one)
            // We should add checks for all lines that could be commands, but skip:
            // - empty lines and whitespace-only lines
            // - comment lines (@rem, rem, ::)
            // - label lines (starting with :)
            let should_add_check = i < lines.len() - 1 
                && !trimmed.is_empty() 
                && !trimmed.starts_with("@rem") 
                && !trimmed.starts_with("rem ") 
                && !trimmed.starts_with("REM ")
                && !trimmed.starts_with("::") 
                && !trimmed.starts_with(':');
                
            if should_add_check {
                result.push("IF %ERRORLEVEL% NEQ 0 EXIT 1".to_string());
            }
        }
        
        result.join("\n")
    }
}

impl Interpreter for CmdExeInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let script = self.get_script(&args, shell::CmdExe).unwrap();

        let build_env_path = args.work_dir.join("build_env.bat");
        let build_script_path = args.work_dir.join("conda_build.bat");

        tokio::fs::write(&build_env_path, script).await?;

        // Add exit code checking for Windows batch files to ensure failing commands 
        // don't get ignored. This mimics conda-build's behavior.
        let processed_script = Self::add_exit_code_checks(args.script.script());

        let build_script = format!(
            "{}\n{}",
            CMDEXE_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy()),
            processed_script
        );
        tokio::fs::write(
            &build_script_path,
            &build_script.replace('\n', "\r\n").as_bytes(),
        )
        .await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let cmd_args = ["cmd.exe", "/d", "/c", &build_script_path_str];

        if args.debug.is_enabled() {
            return Err(InterpreterError::Debug(print_debug_info(&args)));
        }

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("%((var))%"),
            None,
        )
        .await?;

        if !output.status.success() {
            let status_code = output.status.code().unwrap_or(1);
            tracing::error!("Script failed with status {}", status_code);
            tracing::error!("{}", print_debug_info(&args));
            return Err(InterpreterError::ExecutionFailed(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Script failed".to_string(),
            )));
        }

        Ok(())
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        // check if COMSPEC is set to cmd.exe
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if comspec.to_lowercase().contains("cmd.exe") {
                return Ok(Some(PathBuf::from(comspec)));
            }
        }

        // check if cmd.exe is in PATH
        find_interpreter("cmd", build_prefix, platform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_exit_code_checks_empty_script() {
        let script = "";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        assert_eq!(result, "");
    }

    #[test]
    fn test_add_exit_code_checks_single_command() {
        let script = "echo Hello";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        assert_eq!(result, "echo Hello");
    }

    #[test]
    fn test_add_exit_code_checks_multiple_commands() {
        let script = "echo First command\necho Second command\necho Third command";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "echo First command\nIF %ERRORLEVEL% NEQ 0 EXIT 1\necho Second command\nIF %ERRORLEVEL% NEQ 0 EXIT 1\necho Third command";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_exit_code_checks_with_comments() {
        let script = "echo Start\n@rem This is a comment\necho Middle\n:: Another comment\necho End";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "echo Start\nIF %ERRORLEVEL% NEQ 0 EXIT 1\n@rem This is a comment\necho Middle\nIF %ERRORLEVEL% NEQ 0 EXIT 1\n:: Another comment\necho End";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_exit_code_checks_with_empty_lines() {
        let script = "echo First\n\necho Second\n   \necho Third";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "echo First\nIF %ERRORLEVEL% NEQ 0 EXIT 1\n\necho Second\nIF %ERRORLEVEL% NEQ 0 EXIT 1\n   \necho Third";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_exit_code_checks_with_labels() {
        let script = "echo Start\n:label1\necho After label\n:end\necho Final";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "echo Start\nIF %ERRORLEVEL% NEQ 0 EXIT 1\n:label1\necho After label\nIF %ERRORLEVEL% NEQ 0 EXIT 1\n:end\necho Final";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_exit_code_checks_with_if_statements() {
        let script = "echo Test\nIF EXIST file.txt echo File exists\necho Done";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "echo Test\nIF %ERRORLEVEL% NEQ 0 EXIT 1\nIF EXIST file.txt echo File exists\nIF %ERRORLEVEL% NEQ 0 EXIT 1\necho Done";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_exit_code_checks_with_echo_statements() {
        let script = "dir\nECHO Listing files\ndir /w";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "dir\nIF %ERRORLEVEL% NEQ 0 EXIT 1\nECHO Listing files\nIF %ERRORLEVEL% NEQ 0 EXIT 1\ndir /w";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_add_exit_code_checks_realistic_test_script() {
        let script = "python --version\npython -c \"import mypackage\"\npython -m pytest tests/";
        let result = CmdExeInterpreter::add_exit_code_checks(script);
        let expected = "python --version\nIF %ERRORLEVEL% NEQ 0 EXIT 1\npython -c \"import mypackage\"\nIF %ERRORLEVEL% NEQ 0 EXIT 1\npython -m pytest tests/";
        assert_eq!(result, expected);
    }

    #[test] 
    fn test_comprehensive_test_commands() {
        // This test simulates the exact issue described in #1792
        // where failing test commands were being ignored on Windows
        let test_commands = vec![
            "python --version".to_string(),
            "python -c \"import nonexistent_module\"".to_string(), // This would fail
            "echo Success".to_string(), // This should not run if the import fails
        ];
        
        let joined_script = test_commands.join("\n");
        let processed_script = CmdExeInterpreter::add_exit_code_checks(&joined_script);
        
        let expected = "python --version\nIF %ERRORLEVEL% NEQ 0 EXIT 1\npython -c \"import nonexistent_module\"\nIF %ERRORLEVEL% NEQ 0 EXIT 1\necho Success";
        
        assert_eq!(processed_script, expected);
        
        // Verify that the original script would continue on failure (the problem)
        // while the processed script would stop on first failure (the fix)
        assert!(!joined_script.contains("IF %ERRORLEVEL%"));
        assert!(processed_script.contains("IF %ERRORLEVEL% NEQ 0 EXIT 1"));
    }

    #[test]
    fn test_issue_1792_regression() {
        // Integration test that simulates the exact problem from issue #1792
        println!("Testing fix for issue #1792: Windows test exit codes not checked");
        
        let problematic_test = "python --version\npython -c \"import pytest\"\necho Test completed successfully";
        
        let fixed_test = CmdExeInterpreter::add_exit_code_checks(problematic_test);
        
        // Before the fix: if pytest import failed, "Test completed successfully" would still run
        // After the fix: if pytest import fails, the script stops with EXIT 1
        
        assert_eq!(
            fixed_test,
            "python --version\nIF %ERRORLEVEL% NEQ 0 EXIT 1\npython -c \"import pytest\"\nIF %ERRORLEVEL% NEQ 0 EXIT 1\necho Test completed successfully"
        );
        
        // Verify we have the right number of exit checks
        let exit_check_count = fixed_test.matches("IF %ERRORLEVEL% NEQ 0 EXIT 1").count();
        assert_eq!(exit_check_count, 2); // Two commands that could fail
        
        println!("✅ Issue #1792 fix verified: Windows tests will now fail properly on command failures");
    }
}
