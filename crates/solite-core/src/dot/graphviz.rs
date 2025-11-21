use serde::Serialize;
use crate::Runtime;
use crate::sqlite::ValueRefXValue;
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Debug, PartialEq)]
pub struct GraphvizCommand {}

#[derive(Debug)]
struct Column {
    name: String,
    col_type: String,
    is_pk: bool,
    #[allow(dead_code)]
    is_fk: bool,
}

#[derive(Debug)]
struct ForeignKey {
    from_table: String,
    from_column: String,
    to_table: String,
    to_column: String,
    is_unique: bool,
}

impl GraphvizCommand {
    pub fn execute(&self, runtime: &Runtime) -> String {
        // Get all tables
        let tables = self.get_tables(runtime);
        
        // Build column information for each table
        let mut table_columns: HashMap<String, Vec<Column>> = HashMap::new();
        let mut foreign_keys: Vec<ForeignKey> = Vec::new();
        
        for table in &tables {
            let columns = self.get_table_columns(runtime, table);
            let fks = self.get_foreign_keys(runtime, table);
            
            table_columns.insert(table.clone(), columns);
            foreign_keys.extend(fks);
        }
        
        self.generate_dot(&table_columns, &foreign_keys)
    }
    
    fn get_tables(&self, runtime: &Runtime) -> Vec<String> {
        let stmt = runtime
            .connection
            .prepare(
                r#"
                SELECT name
                FROM pragma_table_list
                WHERE "schema" = 'main'
                  AND type IN ('table', 'view')
                  AND name NOT LIKE 'sqlite_%'
                ORDER BY name
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
    
    fn get_table_columns(&self, runtime: &Runtime, table: &str) -> Vec<Column> {
        let query = format!(
            "SELECT name, type, pk FROM pragma_table_info('{}')",
            table.replace("'", "''")
        );
        
        let stmt = runtime
            .connection
            .prepare(&query)
            .unwrap()
            .1
            .unwrap();
        
        let mut columns = vec![];
        while let Ok(Some(row)) = stmt.next() {
            let name = row.get(0).unwrap().as_str().to_owned();
            let col_type = row.get(1).unwrap().as_str().to_owned();
            let pk_value = row.get(2).unwrap();
            let pk = match &pk_value.value {
                ValueRefXValue::Int(i) => *i > 0,
                _ => false,
            };
            
            columns.push(Column {
                name,
                col_type,
                is_pk: pk,
                is_fk: false, // Will be set later
            });
        }
        columns
    }
    
    fn get_foreign_keys(&self, runtime: &Runtime, table: &str) -> Vec<ForeignKey> {
        let query = format!(
            "SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list('{}')",
            table.replace("'", "''")
        );
        
        let stmt = runtime
            .connection
            .prepare(&query)
            .unwrap()
            .1
            .unwrap();
        
        let mut fks = vec![];
        while let Ok(Some(row)) = stmt.next() {
            let from_column = row.get(0).unwrap().as_str().to_owned();
            let to_table = row.get(1).unwrap().as_str().to_owned();
            let to_column = row.get(2).unwrap().as_str().to_owned();
            
            // Check if this is a unique constraint (one-to-one)
            let is_unique = self.is_unique_fk(runtime, table, &from_column);
            
            fks.push(ForeignKey {
                from_table: table.to_owned(),
                from_column,
                to_table,
                to_column,
                is_unique,
            });
        }
        fks
    }
    
    fn is_unique_fk(&self, runtime: &Runtime, table: &str, column: &str) -> bool {
        // Check if the foreign key column is part of a unique index
        let query = format!(
            r#"
            SELECT il.name
            FROM pragma_index_list('{}') AS il
            JOIN pragma_index_info(il.name) AS ii
            WHERE il."unique" = 1
              AND ii.name = '{}'
            "#,
            table.replace("'", "''"),
            column.replace("'", "''")
        );
        
        let stmt = runtime.connection.prepare(&query).unwrap().1.unwrap();
        stmt.next().is_ok()
    }
    
    fn generate_dot(&self, table_columns: &HashMap<String, Vec<Column>>, foreign_keys: &Vec<ForeignKey>) -> String {
        let mut dot = String::from("digraph ERD {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=plaintext];\n\n");
        
        // Mark FK columns
        let mut fk_columns: HashSet<(String, String)> = HashSet::new();
        for fk in foreign_keys {
            fk_columns.insert((fk.from_table.clone(), fk.from_column.clone()));
        }
        
        // Generate nodes for each table
        for (table, columns) in table_columns {
            dot.push_str(&format!("  {} [\n    label=<\n", Self::escape_id(table)));
            dot.push_str("      <TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\" CELLPADDING=\"4\">\n");
            dot.push_str(&format!("        <TR><TD BGCOLOR=\"lightblue\"><B>{}</B></TD></TR>\n", Self::escape_html(table)));
            
            let show_all = columns.len() <= 8;
            let mut shown = 0;
            
            for col in columns {
                let is_fk = fk_columns.contains(&(table.clone(), col.name.clone()));
                
                // Always show PKs and FKs
                if show_all || col.is_pk || is_fk {
                    let mut label = Self::escape_html(&col.name);
                    
                    if col.is_pk {
                        label = format!("<B>{}</B> (PK)", label);
                    } else if is_fk {
                        label = format!("{} (FK)", label);
                    }
                    
                    if !col.col_type.is_empty() {
                        label = format!("{}: {}", label, Self::escape_html(&col.col_type));
                    }
                    
                    dot.push_str(&format!("        <TR><TD ALIGN=\"LEFT\">{}</TD></TR>\n", label));
                    shown += 1;
                }
            }
            
            // Add ellipsis if not showing all columns
            if !show_all && shown < columns.len() {
                dot.push_str("        <TR><TD ALIGN=\"LEFT\">...</TD></TR>\n");
            }
            
            dot.push_str("      </TABLE>\n");
            dot.push_str("    >\n  ];\n\n");
        }
        
        // Generate edges for foreign keys
        for fk in foreign_keys {
            let from_id = Self::escape_id(&fk.from_table);
            let to_id = Self::escape_id(&fk.to_table);
            
            // Different arrow styles based on relationship type
            let arrow_style = if fk.is_unique {
                // One-to-one relationship
                "arrowhead=none, arrowtail=none, dir=both"
            } else {
                // Many-to-one relationship (crow's foot)
                "arrowhead=normal, arrowtail=crow, dir=both"
            };
            
            dot.push_str(&format!(
                "  {} -> {} [label=\"{}.{} → {}.{}\", {}];\n",
                from_id,
                to_id,
                Self::escape_html(&fk.from_table),
                Self::escape_html(&fk.from_column),
                Self::escape_html(&fk.to_table),
                Self::escape_html(&fk.to_column),
                arrow_style
            ));
        }
        
        dot.push_str("}\n");
        dot
    }
    
    fn escape_id(s: &str) -> String {
        format!("\"{}\"", s.replace("\"", "\\\""))
    }
    
    fn escape_html(s: &str) -> String {
        s.replace("&", "&amp;")
            .replace("<", "&lt;")
            .replace(">", "&gt;")
            .replace("\"", "&quot;")
    }
}