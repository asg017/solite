use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct ClearCommand {
}
impl ClearCommand {
    pub fn execute(&self) {
        // clear console
        print!("\x1B[2J\x1B[1;1H");
        println!();
    }
}