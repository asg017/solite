use serde::Serialize;
use crate::Runtime;
  
#[derive(Serialize, Debug, PartialEq)]
pub struct TablesCommand {}
impl TablesCommand {
    pub fn execute(&self, runtime: &Runtime)-> Vec<String> {
        let stmt = runtime
            .connection
            .prepare(
                r#"
                select name
                from pragma_table_list
                where "schema" = 'main'
                  and type in ('table', 'view')
                  and name not like 'sqlite_%'
                order by name;
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