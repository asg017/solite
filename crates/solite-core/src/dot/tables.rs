use serde::Serialize;
use crate::Runtime;

#[derive(Serialize, Debug, PartialEq)]
pub struct TablesCommand {
  pub schema: Option<String>,
}
impl TablesCommand {
    pub fn execute(&self, runtime: &Runtime)-> Vec<String> {
        let stmt = runtime
            .connection
            .prepare(
                r#"
                select name
                from pragma_table_list
                where "schema" = COALESCE(?, 'main')
                  and type in ('table', 'view')
                  and name not like 'sqlite_%'
                order by name;
                "#,
            )
            .unwrap()
            .1
            .unwrap();
        if let Some(schema) = &self.schema {
            stmt.bind_text(1, schema.as_str());
        }
        let mut tables = vec![];
        while let Ok(Some(row)) = stmt.next() {
            tables.push(row.get(0).unwrap().as_str().to_owned());
        }

        tables
    }
}