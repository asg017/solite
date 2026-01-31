//! Shell command execution.
//!
//! This module implements the `.sh` command which executes shell commands.
//!
//! # Usage
//!
//! ```sql
//! .sh ls -la              -- Run command in foreground
//! .sh & sleep 10          -- Run command in background (prefix with &)
//! ```
//!
//! # Modes
//!
//! - **Foreground**: Command output is streamed back via a channel
//! - **Background**: Command runs detached, output is discarded
//!
//! # Platform
//!
//! Commands are executed via `sh -c` on Unix systems.

use crate::dot::DotError;
use serde::Serialize;
use std::{
    io::{BufRead, BufReader},
    process::Child,
    sync::mpsc::Receiver,
    thread,
};

/// Command to execute a shell command.
#[derive(Serialize, Debug, PartialEq)]
pub struct ShellCommand {
    /// The shell command to execute.
    pub command: String,
}

/// Result of shell command execution.
pub enum ShellResult {
    /// Foreground command with streaming output.
    Stream(Receiver<String>),
    /// Background command with process handle.
    Background(Child),
}

impl ShellCommand {
    /// Execute the shell command.
    ///
    /// # Returns
    ///
    /// - `ShellResult::Stream` for foreground commands with output receiver
    /// - `ShellResult::Background` for background commands with process handle
    ///
    /// # Errors
    ///
    /// Returns `DotError::Io` if the command cannot be spawned.
    pub fn execute(&self) -> Result<ShellResult, DotError> {
        let command = self.command.clone();

        // Background command (prefix with &)
        if let Some(bg_command) = command.strip_prefix('&') {
            let bg_command = bg_command.trim();
            let child = std::process::Command::new("sh")
                .arg("-c")
                .arg(bg_command)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            return Ok(ShellResult::Background(child));
        }

        // Foreground command with streaming output
        let (tx, rx) = std::sync::mpsc::channel::<String>();

        std::thread::spawn(move || {
            let child_result = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();

            let mut child = match child_result {
                Ok(child) => child,
                Err(e) => {
                    let _ = tx.send(format!("Error spawning command: {}", e));
                    return;
                }
            };

            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => return,
            };
            let stderr = match child.stderr.take() {
                Some(stderr) => stderr,
                None => return,
            };

            let tx_clone = tx.clone();

            // Handle stdout in a separate thread
            let stdout_handle = thread::spawn(move || {
                let reader = BufReader::with_capacity(1, stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if tx_clone.send(line).is_err() {
                        break;
                    }
                }
            });

            // Handle stderr in a separate thread
            let stderr_handle = thread::spawn(move || {
                let reader = BufReader::with_capacity(1, stderr);
                for line in reader.lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });

            // Wait for both threads to complete
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            let _ = child.wait();
        });

        Ok(ShellResult::Stream(rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_command_parse_foreground() {
        let cmd = ShellCommand {
            command: "echo hello".to_string(),
        };
        // Just test that we can create and execute without error
        let result = cmd.execute();
        assert!(result.is_ok());

        match result.unwrap() {
            ShellResult::Stream(_) => (), // Expected
            ShellResult::Background(_) => panic!("Expected foreground command"),
        }
    }

    #[test]
    fn test_shell_command_parse_background() {
        let cmd = ShellCommand {
            command: "& sleep 0".to_string(),
        };

        let result = cmd.execute();
        assert!(result.is_ok());

        match result.unwrap() {
            ShellResult::Background(mut child) => {
                // Wait for the child to complete
                let status = child.wait();
                assert!(status.is_ok());
            }
            ShellResult::Stream(_) => panic!("Expected background command"),
        }
    }

    #[test]
    fn test_shell_command_struct() {
        let cmd = ShellCommand {
            command: "ls".to_string(),
        };
        assert_eq!(cmd.command, "ls");
    }
}
