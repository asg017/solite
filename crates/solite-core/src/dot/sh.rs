use serde::Serialize;
use std::{
     io::{BufRead, BufReader}, sync::mpsc::Receiver
};

#[derive(Serialize, Debug, PartialEq)]
pub struct ShellCommand {
    pub command: String,
}
impl ShellCommand {
    pub fn execute(&self) -> Receiver<String> {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let command = self.command.clone();
        std::thread::spawn(move || {
            let mut child = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let reader = BufReader::with_capacity(1, stdout); //new(stdout);
            let mut lines = reader.lines();
            while let Some(Ok(line)) = lines.next() {
                tx.send(line).unwrap();
            }
            let _ = child.wait();
        });
        return rx;
    }
}