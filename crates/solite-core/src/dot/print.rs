use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct PrintCommand {
    pub message: String,
}
impl PrintCommand {
    pub fn execute(&self) {
        println!("{}", self.message);
    }
}