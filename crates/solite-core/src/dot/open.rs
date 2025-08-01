use serde::Serialize;
use crate::{Runtime, Connection};
use solite_stdlib::solite_stdlib_init;

#[derive(Serialize, Debug, PartialEq)]
pub struct OpenCommand {
    pub path: String,
}
impl OpenCommand {
    pub fn execute(&self, runtime: &mut Runtime) {
        runtime.connection = Connection::open(&self.path).unwrap();
        unsafe {
            solite_stdlib_init(
                runtime.connection.db(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }
}