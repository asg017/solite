use serde::Serialize;
use crate::Runtime;

#[derive(Serialize, Debug, PartialEq)]
pub struct SchemaCommand {}
impl SchemaCommand {
    pub fn execute(&self, runtime: &Runtime)-> Vec<String> {
        let stmt = runtime
            .connection
            .prepare(
                r#"
                select sql
                from sqlite_master
                "#,
            )
            .unwrap()
            .1
            .unwrap();
        let mut tables = vec![];
        while let Ok(Some(row)) = stmt.next() {
            tables.push(row.get(0).unwrap().as_str().to_owned());
        }

        tables
    }
}