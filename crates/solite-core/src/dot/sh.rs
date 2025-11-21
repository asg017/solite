use serde::Serialize;
use std::{
     io::{BufRead, BufReader}, process::Child, sync::mpsc::Receiver, thread
};

#[derive(Serialize, Debug, PartialEq)]
pub struct ShellCommand {
    pub command: String,
}

pub enum ShellResult {
  Stream(Receiver<String>),
  Background(Child),
}
impl ShellCommand {
    pub fn execute(&self) -> ShellResult {
        let command = self.command.clone();
        if let Some(command) = command.strip_prefix("&") {
            let command = command.trim();
            let child = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            return ShellResult::Background(child);
        }
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        
        std::thread::spawn(move || {
            let mut child = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");
            
            let tx_clone = tx.clone();
            
            // Handle stdout in a separate thread
            let stdout_handle = thread::spawn(move || {
                let reader = BufReader::with_capacity(1, stdout);
                let mut lines = reader.lines();
                while let Some(Ok(line)) = lines.next() {
                    if tx_clone.send(line).is_err() {
                        break;
                    }
                }
            });
            
            // Handle stderr in a separate thread
            let stderr_handle = thread::spawn(move || {
                let reader = BufReader::with_capacity(1, stderr);
                let mut lines = reader.lines();
                while let Some(Ok(line)) = lines.next() {
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
        ShellResult::Stream(rx)
    }
}