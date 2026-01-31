//! JSON-based schema loading.
//!
//! This module provides schema loading from JSON files, allowing
//! schema information to be provided without a live database connection.
//! This is particularly useful for WASM/browser environments where
//! rusqlite is not available.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JsonSchemaError {
    #[error("JSON parse error: {0}")]
    ParseError(String),
    #[error("Invalid schema: {0}")]
    ValidationError(String),
    #[error("JSON serialization error: {0}")]
    SerializeError(String),
}

/// JSON representation of a database schema
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct JsonSchema {
    #[serde(default)]
    pub tables: Vec<JsonTable>,
    #[serde(default)]
    pub views: Vec<JsonView>,
    #[serde(default)]
    pub indexes: Vec<JsonIndex>,
    #[serde(default)]
    pub triggers: Vec<JsonTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonTable {
    pub name: String,
    pub columns: Vec<JsonColumn>,
    #[serde(default)]
    pub without_rowid: bool,
    /// Table description from sqlite-docs `--!` comments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Additional tags from sqlite-docs (e.g., @details, @source)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonColumn {
    pub name: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub not_null: bool,
    /// Column description from sqlite-docs `---` comments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Example value(s) from `@example` tag
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// Additional tags from sqlite-docs (e.g., @value, @source)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonView {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonIndex {
    pub name: String,
    pub table_name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonTrigger {
    pub name: String,
    pub table_name: String,
    pub event: String, // "INSERT", "UPDATE", "DELETE"
}

impl JsonSchema {
    /// Create a new empty JsonSchema
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a JSON string into a JsonSchema
    pub fn from_json(json: &str) -> Result<Self, JsonSchemaError> {
        serde_json::from_str(json).map_err(|e| JsonSchemaError::ParseError(e.to_string()))
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> Result<String, JsonSchemaError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| JsonSchemaError::SerializeError(e.to_string()))
    }

    /// Serialize to compact JSON string (no pretty printing)
    pub fn to_json_compact(&self) -> Result<String, JsonSchemaError> {
        serde_json::to_string(self).map_err(|e| JsonSchemaError::SerializeError(e.to_string()))
    }

    /// Validate the schema for consistency
    pub fn validate(&self) -> Result<(), JsonSchemaError> {
        // Check for duplicate table names
        let mut table_names = std::collections::HashSet::new();
        for table in &self.tables {
            let lower = table.name.to_lowercase();
            if !table_names.insert(lower) {
                return Err(JsonSchemaError::ValidationError(format!(
                    "Duplicate table name: {}",
                    table.name
                )));
            }

            // Check for duplicate column names within each table
            let mut col_names = std::collections::HashSet::new();
            for col in &table.columns {
                let col_lower = col.name.to_lowercase();
                if !col_names.insert(col_lower) {
                    return Err(JsonSchemaError::ValidationError(format!(
                        "Duplicate column '{}' in table '{}'",
                        col.name, table.name
                    )));
                }
            }
        }

        // Check for duplicate view names
        let mut view_names = std::collections::HashSet::new();
        for view in &self.views {
            let lower = view.name.to_lowercase();
            if !view_names.insert(lower) {
                return Err(JsonSchemaError::ValidationError(format!(
                    "Duplicate view name: {}",
                    view.name
                )));
            }
        }

        // Check for duplicate index names
        let mut index_names = std::collections::HashSet::new();
        for index in &self.indexes {
            let lower = index.name.to_lowercase();
            if !index_names.insert(lower) {
                return Err(JsonSchemaError::ValidationError(format!(
                    "Duplicate index name: {}",
                    index.name
                )));
            }
        }

        // Check for duplicate trigger names
        let mut trigger_names = std::collections::HashSet::new();
        for trigger in &self.triggers {
            let lower = trigger.name.to_lowercase();
            if !trigger_names.insert(lower) {
                return Err(JsonSchemaError::ValidationError(format!(
                    "Duplicate trigger name: {}",
                    trigger.name
                )));
            }

            // Validate trigger event
            let event_upper = trigger.event.to_uppercase();
            if event_upper != "INSERT" && event_upper != "UPDATE" && event_upper != "DELETE" {
                return Err(JsonSchemaError::ValidationError(format!(
                    "Invalid trigger event '{}' for trigger '{}'. Must be INSERT, UPDATE, or DELETE",
                    trigger.event, trigger.name
                )));
            }
        }

        Ok(())
    }

    /// Add a table to the schema
    pub fn add_table(&mut self, table: JsonTable) {
        self.tables.push(table);
    }

    /// Add a view to the schema
    pub fn add_view(&mut self, view: JsonView) {
        self.views.push(view);
    }

    /// Add an index to the schema
    pub fn add_index(&mut self, index: JsonIndex) {
        self.indexes.push(index);
    }

    /// Add a trigger to the schema
    pub fn add_trigger(&mut self, trigger: JsonTrigger) {
        self.triggers.push(trigger);
    }
}

impl JsonTable {
    /// Create a new table with the given name and columns
    pub fn new(name: impl Into<String>, columns: Vec<JsonColumn>) -> Self {
        Self {
            name: name.into(),
            columns,
            without_rowid: false,
            description: None,
            tags: None,
        }
    }

    /// Create a new WITHOUT ROWID table
    pub fn without_rowid(mut self) -> Self {
        self.without_rowid = true;
        self
    }

    /// Set the table description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl JsonColumn {
    /// Create a new column with just a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            r#type: None,
            primary_key: false,
            not_null: false,
            description: None,
            example: None,
            tags: None,
        }
    }

    /// Create a new column with a name and type
    pub fn with_type(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            r#type: Some(type_name.into()),
            primary_key: false,
            not_null: false,
            description: None,
            example: None,
            tags: None,
        }
    }

    /// Mark this column as primary key
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self
    }

    /// Mark this column as NOT NULL
    pub fn not_null(mut self) -> Self {
        self.not_null = true;
        self
    }

    /// Set the column description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the example value
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.example = Some(example.into());
        self
    }
}

impl JsonView {
    /// Create a new view with the given name and columns
    pub fn new(name: impl Into<String>, columns: Vec<String>) -> Self {
        Self {
            name: name.into(),
            columns,
        }
    }
}

impl JsonIndex {
    /// Create a new index
    pub fn new(
        name: impl Into<String>,
        table_name: impl Into<String>,
        columns: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            table_name: table_name.into(),
            columns,
            unique: false,
        }
    }

    /// Mark this index as UNIQUE
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
}

impl JsonTrigger {
    /// Create a new trigger
    pub fn new(
        name: impl Into<String>,
        table_name: impl Into<String>,
        event: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            table_name: table_name.into(),
            event: event.into(),
        }
    }
}

// ============================================================================
// Conversion to analyzer Schema
// ============================================================================

impl JsonSchema {
    /// Convert this JSON schema to the analyzer's Schema type.
    ///
    /// This allows JSON-loaded schemas to be used for SQL analysis and validation.
    pub fn to_analyzer_schema(&self) -> solite_analyzer::Schema {
        use solite_analyzer::{Schema, TriggerEventType};
        use solite_ast::DocComment;
        use std::collections::HashMap;

        let mut schema = Schema::new();

        // Add tables with documentation
        for table in &self.tables {
            let columns: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();

            // Convert table documentation
            let table_doc = if table.description.is_some() || table.tags.is_some() {
                let mut doc = DocComment::new();
                if let Some(ref desc) = table.description {
                    doc.description = desc.clone();
                }
                if let Some(ref tags) = table.tags {
                    doc.tags = tags.clone();
                }
                Some(doc)
            } else {
                None
            };

            // Convert column documentation
            let mut column_docs: HashMap<String, DocComment> = HashMap::new();
            for col in &table.columns {
                if col.description.is_some() || col.example.is_some() || col.tags.is_some() {
                    let mut doc = DocComment::new();
                    if let Some(ref desc) = col.description {
                        doc.description = desc.clone();
                    }
                    if let Some(ref example) = col.example {
                        doc.tags.insert("example".to_string(), vec![example.clone()]);
                    }
                    if let Some(ref tags) = col.tags {
                        for (key, values) in tags {
                            doc.tags.entry(key.clone()).or_default().extend(values.clone());
                        }
                    }
                    column_docs.insert(col.name.to_lowercase(), doc);
                }
            }

            schema.add_table_with_doc(&table.name, columns, table.without_rowid, table_doc, column_docs);
        }

        // Add views
        for view in &self.views {
            schema.add_view(&view.name, view.columns.clone());
        }

        // Add indexes
        for index in &self.indexes {
            schema.add_index(&index.name, &index.table_name, index.columns.clone(), index.unique);
        }

        // Add triggers
        for trigger in &self.triggers {
            let event = match trigger.event.to_uppercase().as_str() {
                "INSERT" => TriggerEventType::Insert,
                "UPDATE" => TriggerEventType::Update,
                "DELETE" => TriggerEventType::Delete,
                _ => TriggerEventType::Insert, // fallback, validation should catch this
            };
            schema.add_trigger(&trigger.name, &trigger.table_name, event);
        }

        schema
    }
}

impl From<JsonSchema> for solite_analyzer::Schema {
    fn from(json_schema: JsonSchema) -> Self {
        json_schema.to_analyzer_schema()
    }
}

impl From<&JsonSchema> for solite_analyzer::Schema {
    fn from(json_schema: &JsonSchema) -> Self {
        json_schema.to_analyzer_schema()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_json() {
        let json = r#"{
            "tables": [
                {
                    "name": "users",
                    "columns": [
                        {"name": "id", "type": "INTEGER", "primary_key": true, "not_null": true},
                        {"name": "name", "type": "TEXT"}
                    ]
                }
            ],
            "views": [
                {"name": "v_users", "columns": ["id", "name"]}
            ],
            "indexes": [
                {"name": "idx_users_name", "table_name": "users", "columns": ["name"]}
            ],
            "triggers": [
                {"name": "trg_audit", "table_name": "users", "event": "INSERT"}
            ]
        }"#;

        let schema = JsonSchema::from_json(json).unwrap();

        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "users");
        assert_eq!(schema.tables[0].columns.len(), 2);
        assert_eq!(schema.tables[0].columns[0].name, "id");
        assert_eq!(
            schema.tables[0].columns[0].r#type,
            Some("INTEGER".to_string())
        );
        assert!(schema.tables[0].columns[0].primary_key);
        assert!(schema.tables[0].columns[0].not_null);
        assert_eq!(schema.tables[0].columns[1].name, "name");
        assert!(!schema.tables[0].columns[1].primary_key);
        assert!(!schema.tables[0].columns[1].not_null);

        assert_eq!(schema.views.len(), 1);
        assert_eq!(schema.views[0].name, "v_users");
        assert_eq!(schema.views[0].columns, vec!["id", "name"]);

        assert_eq!(schema.indexes.len(), 1);
        assert_eq!(schema.indexes[0].name, "idx_users_name");
        assert_eq!(schema.indexes[0].table_name, "users");
        assert_eq!(schema.indexes[0].columns, vec!["name"]);
        assert!(!schema.indexes[0].unique);

        assert_eq!(schema.triggers.len(), 1);
        assert_eq!(schema.triggers[0].name, "trg_audit");
        assert_eq!(schema.triggers[0].table_name, "users");
        assert_eq!(schema.triggers[0].event, "INSERT");
    }

    #[test]
    fn test_parse_missing_optional_fields() {
        let json = r#"{
            "tables": [
                {
                    "name": "simple",
                    "columns": [
                        {"name": "a"}
                    ]
                }
            ]
        }"#;

        let schema = JsonSchema::from_json(json).unwrap();

        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "simple");
        assert!(!schema.tables[0].without_rowid);
        assert_eq!(schema.tables[0].columns.len(), 1);
        assert_eq!(schema.tables[0].columns[0].name, "a");
        assert!(schema.tables[0].columns[0].r#type.is_none());
        assert!(!schema.tables[0].columns[0].primary_key);
        assert!(!schema.tables[0].columns[0].not_null);

        // Optional collections should be empty
        assert!(schema.views.is_empty());
        assert!(schema.indexes.is_empty());
        assert!(schema.triggers.is_empty());
    }

    #[test]
    fn test_parse_empty_schema() {
        let json = "{}";
        let schema = JsonSchema::from_json(json).unwrap();

        assert!(schema.tables.is_empty());
        assert!(schema.views.is_empty());
        assert!(schema.indexes.is_empty());
        assert!(schema.triggers.is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "{ not valid json }";
        let result = JsonSchema::from_json(json);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), JsonSchemaError::ParseError(_)));
    }

    #[test]
    fn test_parse_wrong_type() {
        let json = r#"{"tables": "not an array"}"#;
        let result = JsonSchema::from_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_required_fields() {
        // Table without name should fail
        let json = r#"{"tables": [{"columns": [{"name": "a"}]}]}"#;
        let result = JsonSchema::from_json(json);
        assert!(result.is_err());

        // Column without name should fail
        let json = r#"{"tables": [{"name": "t", "columns": [{"type": "INTEGER"}]}]}"#;
        let result = JsonSchema::from_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_round_trip_serialization() {
        let original = JsonSchema {
            tables: vec![JsonTable {
                name: "users".to_string(),
                columns: vec![
                    JsonColumn {
                        name: "id".to_string(),
                        r#type: Some("INTEGER".to_string()),
                        primary_key: true,
                        not_null: true,
                        description: None,
                        example: None,
                        tags: None,
                    },
                    JsonColumn {
                        name: "email".to_string(),
                        r#type: Some("TEXT".to_string()),
                        primary_key: false,
                        not_null: false,
                        description: None,
                        example: None,
                        tags: None,
                    },
                ],
                without_rowid: false,
                description: None,
                tags: None,
            }],
            views: vec![JsonView {
                name: "v_users".to_string(),
                columns: vec!["id".to_string(), "email".to_string()],
            }],
            indexes: vec![JsonIndex {
                name: "idx_email".to_string(),
                table_name: "users".to_string(),
                columns: vec!["email".to_string()],
                unique: true,
            }],
            triggers: vec![JsonTrigger {
                name: "trg_log".to_string(),
                table_name: "users".to_string(),
                event: "INSERT".to_string(),
            }],
        };

        let json = original.to_json().unwrap();
        let parsed = JsonSchema::from_json(&json).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn test_validate_valid_schema() {
        let schema = JsonSchema {
            tables: vec![JsonTable::new(
                "users",
                vec![
                    JsonColumn::new("id").primary_key(),
                    JsonColumn::new("name"),
                ],
            )],
            views: vec![JsonView::new("v_users", vec!["id".into(), "name".into()])],
            indexes: vec![JsonIndex::new("idx_name", "users", vec!["name".into()])],
            triggers: vec![JsonTrigger::new("trg_audit", "users", "INSERT")],
        };

        assert!(schema.validate().is_ok());
    }

    #[test]
    fn test_validate_duplicate_table() {
        let schema = JsonSchema {
            tables: vec![
                JsonTable::new("Users", vec![JsonColumn::new("a")]),
                JsonTable::new("users", vec![JsonColumn::new("b")]), // duplicate (case-insensitive)
            ],
            ..Default::default()
        };

        let result = schema.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Duplicate table"));
    }

    #[test]
    fn test_validate_duplicate_column() {
        let schema = JsonSchema {
            tables: vec![JsonTable::new(
                "t",
                vec![
                    JsonColumn::new("ID"),
                    JsonColumn::new("id"), // duplicate (case-insensitive)
                ],
            )],
            ..Default::default()
        };

        let result = schema.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Duplicate column"));
    }

    #[test]
    fn test_validate_duplicate_view() {
        let schema = JsonSchema {
            views: vec![
                JsonView::new("V", vec!["a".into()]),
                JsonView::new("v", vec!["b".into()]), // duplicate
            ],
            ..Default::default()
        };

        let result = schema.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate view"));
    }

    #[test]
    fn test_validate_duplicate_index() {
        let schema = JsonSchema {
            indexes: vec![
                JsonIndex::new("IDX", "t", vec!["a".into()]),
                JsonIndex::new("idx", "t", vec!["b".into()]), // duplicate
            ],
            ..Default::default()
        };

        let result = schema.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate index"));
    }

    #[test]
    fn test_validate_duplicate_trigger() {
        let schema = JsonSchema {
            triggers: vec![
                JsonTrigger::new("TRG", "t", "INSERT"),
                JsonTrigger::new("trg", "t", "DELETE"), // duplicate
            ],
            ..Default::default()
        };

        let result = schema.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Duplicate trigger"));
    }

    #[test]
    fn test_validate_invalid_trigger_event() {
        let schema = JsonSchema {
            triggers: vec![JsonTrigger::new("trg", "t", "INVALID_EVENT")],
            ..Default::default()
        };

        let result = schema.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid trigger event"));
    }

    #[test]
    fn test_validate_trigger_events_case_insensitive() {
        // All valid event cases should pass
        for event in ["INSERT", "insert", "Insert", "UPDATE", "update", "DELETE", "delete"] {
            let schema = JsonSchema {
                triggers: vec![JsonTrigger::new("trg", "t", event)],
                ..Default::default()
            };
            assert!(
                schema.validate().is_ok(),
                "Event '{}' should be valid",
                event
            );
        }
    }

    #[test]
    fn test_builder_api() {
        let mut schema = JsonSchema::new();
        schema.add_table(JsonTable::new(
            "users",
            vec![
                JsonColumn::with_type("id", "INTEGER")
                    .primary_key()
                    .not_null(),
                JsonColumn::with_type("email", "TEXT").not_null(),
            ],
        ));
        schema.add_table(JsonTable::new("items", vec![JsonColumn::new("sku")]).without_rowid());
        schema.add_view(JsonView::new(
            "v_users",
            vec!["id".into(), "email".into()],
        ));
        schema.add_index(JsonIndex::new("idx_email", "users", vec!["email".into()]).unique());
        schema.add_trigger(JsonTrigger::new("trg_audit", "users", "INSERT"));

        assert_eq!(schema.tables.len(), 2);
        assert!(schema.tables[0].columns[0].primary_key);
        assert!(schema.tables[0].columns[0].not_null);
        assert!(schema.tables[1].without_rowid);
        assert_eq!(schema.views.len(), 1);
        assert_eq!(schema.indexes.len(), 1);
        assert!(schema.indexes[0].unique);
        assert_eq!(schema.triggers.len(), 1);
    }

    #[test]
    fn test_to_json_compact() {
        let schema = JsonSchema {
            tables: vec![JsonTable::new("t", vec![JsonColumn::new("a")])],
            ..Default::default()
        };

        let compact = schema.to_json_compact().unwrap();
        // Compact JSON should not have newlines
        assert!(!compact.contains('\n'));
        // But should still be valid JSON
        let parsed = JsonSchema::from_json(&compact).unwrap();
        assert_eq!(schema, parsed);
    }

    #[test]
    fn test_schema_with_all_types() {
        let json = r#"{
            "tables": [
                {
                    "name": "products",
                    "columns": [
                        {"name": "id", "type": "INTEGER", "primary_key": true, "not_null": true},
                        {"name": "name", "type": "TEXT", "not_null": true},
                        {"name": "price", "type": "REAL"},
                        {"name": "data", "type": "BLOB"}
                    ],
                    "without_rowid": true
                },
                {
                    "name": "orders",
                    "columns": [
                        {"name": "id", "type": "INTEGER", "primary_key": true},
                        {"name": "product_id", "type": "INTEGER", "not_null": true},
                        {"name": "quantity", "type": "INTEGER"}
                    ]
                }
            ],
            "views": [
                {"name": "v_order_summary", "columns": ["order_id", "product_name", "total"]}
            ],
            "indexes": [
                {"name": "idx_orders_product", "table_name": "orders", "columns": ["product_id"]},
                {"name": "idx_products_name", "table_name": "products", "columns": ["name"], "unique": true}
            ],
            "triggers": [
                {"name": "trg_orders_insert", "table_name": "orders", "event": "INSERT"},
                {"name": "trg_orders_update", "table_name": "orders", "event": "UPDATE"},
                {"name": "trg_orders_delete", "table_name": "orders", "event": "DELETE"}
            ]
        }"#;

        let schema = JsonSchema::from_json(json).unwrap();
        assert!(schema.validate().is_ok());

        assert_eq!(schema.tables.len(), 2);
        assert!(schema.tables[0].without_rowid);
        assert!(!schema.tables[1].without_rowid);

        assert_eq!(schema.views.len(), 1);
        assert_eq!(schema.indexes.len(), 2);
        assert!(!schema.indexes[0].unique);
        assert!(schema.indexes[1].unique);

        assert_eq!(schema.triggers.len(), 3);
    }

    #[test]
    fn test_empty_collections_serialize() {
        let schema = JsonSchema::default();
        let json = schema.to_json().unwrap();
        let parsed = JsonSchema::from_json(&json).unwrap();
        assert_eq!(schema, parsed);
    }

    #[test]
    fn test_unicode_names() {
        let schema = JsonSchema {
            tables: vec![JsonTable::new(
                "utilisateurs",
                vec![
                    JsonColumn::with_type("identifiant", "INTEGER"),
                    JsonColumn::with_type("nom", "TEXT"),
                ],
            )],
            views: vec![JsonView::new(
                "vue_utilisateurs",
                vec!["identifiant".into(), "nom".into()],
            )],
            ..Default::default()
        };

        let json = schema.to_json().unwrap();
        let parsed = JsonSchema::from_json(&json).unwrap();
        assert_eq!(schema, parsed);
    }

    #[test]
    fn test_special_characters_in_names() {
        // SQLite allows special characters in quoted identifiers
        let schema = JsonSchema {
            tables: vec![JsonTable::new(
                "table with spaces",
                vec![JsonColumn::new("column-with-dashes")],
            )],
            ..Default::default()
        };

        let json = schema.to_json().unwrap();
        let parsed = JsonSchema::from_json(&json).unwrap();
        assert_eq!(schema, parsed);
    }

    // ========================================
    // Tests for conversion to analyzer Schema
    // ========================================

    #[test]
    fn test_convert_to_analyzer_schema_tables() {
        let json_schema = JsonSchema {
            tables: vec![JsonTable::new(
                "users",
                vec![
                    JsonColumn::with_type("id", "INTEGER").primary_key(),
                    JsonColumn::with_type("name", "TEXT"),
                ],
            )],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        assert!(schema.has_table("users"));
        assert!(schema.has_table("USERS")); // case insensitive
        let cols = schema.columns_for_table("users").unwrap();
        assert_eq!(cols.len(), 2);
        assert!(cols.contains(&"id".to_string()));
        assert!(cols.contains(&"name".to_string()));
    }

    #[test]
    fn test_convert_to_analyzer_schema_without_rowid() {
        let json_schema = JsonSchema {
            tables: vec![JsonTable::new("items", vec![JsonColumn::new("sku")]).without_rowid()],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        assert!(schema.has_table("items"));
        let table = schema.get_table("items").unwrap();
        assert!(table.without_rowid);
    }

    #[test]
    fn test_convert_to_analyzer_schema_views() {
        let json_schema = JsonSchema {
            views: vec![JsonView::new(
                "v_users",
                vec!["id".into(), "name".into()],
            )],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        assert!(schema.has_view("v_users"));
        assert!(schema.has_view("V_USERS")); // case insensitive
        let cols = schema.columns_for_view("v_users").unwrap();
        assert_eq!(cols, &["id".to_string(), "name".to_string()]);
    }

    #[test]
    fn test_convert_to_analyzer_schema_indexes() {
        let json_schema = JsonSchema {
            indexes: vec![
                JsonIndex::new("idx_users_name", "users", vec!["name".into()]),
                JsonIndex::new("idx_users_email", "users", vec!["email".into()]).unique(),
            ],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        assert!(schema.has_index("idx_users_name"));
        assert!(schema.has_index("idx_users_email"));

        let idx1 = schema.get_index("idx_users_name").unwrap();
        assert_eq!(idx1.table_name, "users");
        assert_eq!(idx1.columns, vec!["name".to_string()]);
        assert!(!idx1.is_unique);

        let idx2 = schema.get_index("idx_users_email").unwrap();
        assert!(idx2.is_unique);
    }

    #[test]
    fn test_convert_to_analyzer_schema_triggers() {
        use solite_analyzer::TriggerEventType;

        let json_schema = JsonSchema {
            triggers: vec![
                JsonTrigger::new("trg_insert", "users", "INSERT"),
                JsonTrigger::new("trg_update", "users", "UPDATE"),
                JsonTrigger::new("trg_delete", "users", "DELETE"),
            ],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        assert!(schema.has_trigger("trg_insert"));
        assert!(schema.has_trigger("trg_update"));
        assert!(schema.has_trigger("trg_delete"));

        let trg1 = schema.get_trigger("trg_insert").unwrap();
        assert_eq!(trg1.event, TriggerEventType::Insert);

        let trg2 = schema.get_trigger("trg_update").unwrap();
        assert_eq!(trg2.event, TriggerEventType::Update);

        let trg3 = schema.get_trigger("trg_delete").unwrap();
        assert_eq!(trg3.event, TriggerEventType::Delete);
    }

    #[test]
    fn test_convert_to_analyzer_schema_trigger_case_insensitive() {
        use solite_analyzer::TriggerEventType;

        let json_schema = JsonSchema {
            triggers: vec![
                JsonTrigger::new("trg1", "t", "insert"),
                JsonTrigger::new("trg2", "t", "Insert"),
                JsonTrigger::new("trg3", "t", "INSERT"),
            ],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        // All should be Insert regardless of case
        assert_eq!(
            schema.get_trigger("trg1").unwrap().event,
            TriggerEventType::Insert
        );
        assert_eq!(
            schema.get_trigger("trg2").unwrap().event,
            TriggerEventType::Insert
        );
        assert_eq!(
            schema.get_trigger("trg3").unwrap().event,
            TriggerEventType::Insert
        );
    }

    #[test]
    fn test_convert_to_analyzer_schema_full() {
        let json_schema = JsonSchema {
            tables: vec![
                JsonTable::new(
                    "users",
                    vec![
                        JsonColumn::with_type("id", "INTEGER").primary_key().not_null(),
                        JsonColumn::with_type("email", "TEXT").not_null(),
                    ],
                ),
                JsonTable::new("items", vec![JsonColumn::new("sku")]).without_rowid(),
            ],
            views: vec![JsonView::new("v_users", vec!["id".into(), "email".into()])],
            indexes: vec![JsonIndex::new("idx_email", "users", vec!["email".into()]).unique()],
            triggers: vec![JsonTrigger::new("trg_audit", "users", "INSERT")],
        };

        let schema = json_schema.to_analyzer_schema();

        // Verify all objects exist
        assert!(schema.has_table("users"));
        assert!(schema.has_table("items"));
        assert!(schema.has_view("v_users"));
        assert!(schema.has_index("idx_email"));
        assert!(schema.has_trigger("trg_audit"));

        // Verify details
        assert!(schema.get_table("items").unwrap().without_rowid);
        assert!(schema.get_index("idx_email").unwrap().is_unique);
    }

    #[test]
    fn test_convert_empty_schema() {
        let json_schema = JsonSchema::default();
        let schema = json_schema.to_analyzer_schema();

        // Empty schema should have no objects
        assert_eq!(schema.table_names().count(), 0);
        assert_eq!(schema.view_names().count(), 0);
        assert_eq!(schema.index_names().count(), 0);
        assert_eq!(schema.trigger_names().count(), 0);
    }

    #[test]
    fn test_from_trait_conversion() {
        let json_schema = JsonSchema {
            tables: vec![JsonTable::new("t", vec![JsonColumn::new("a")])],
            ..Default::default()
        };

        // Test From<JsonSchema>
        let schema1: solite_analyzer::Schema = json_schema.clone().into();
        assert!(schema1.has_table("t"));

        // Test From<&JsonSchema>
        let schema2: solite_analyzer::Schema = (&json_schema).into();
        assert!(schema2.has_table("t"));
    }

    #[test]
    fn test_json_schema_with_documentation() {
        // Test parsing JSON with documentation fields
        let json = r#"{
            "tables": [{
                "name": "students",
                "description": "All students at Foo University.",
                "tags": {"details": ["https://foo.edu/students"]},
                "columns": [
                    {
                        "name": "student_id",
                        "type": "TEXT",
                        "primary_key": true,
                        "description": "Student ID assigned at orientation",
                        "example": "'S10483'"
                    },
                    {
                        "name": "name",
                        "type": "TEXT",
                        "description": "Full name of student"
                    }
                ]
            }]
        }"#;

        let schema = JsonSchema::from_json(json).unwrap();

        // Check table doc fields
        assert_eq!(schema.tables[0].name, "students");
        assert_eq!(schema.tables[0].description, Some("All students at Foo University.".to_string()));
        assert!(schema.tables[0].tags.is_some());

        // Check column doc fields
        assert_eq!(schema.tables[0].columns[0].description, Some("Student ID assigned at orientation".to_string()));
        assert_eq!(schema.tables[0].columns[0].example, Some("'S10483'".to_string()));
        assert_eq!(schema.tables[0].columns[1].description, Some("Full name of student".to_string()));
    }

    #[test]
    fn test_json_docs_convert_to_analyzer_schema() {
        let json_schema = JsonSchema {
            tables: vec![JsonTable::new(
                "students",
                vec![
                    JsonColumn::new("student_id")
                        .with_description("Student ID")
                        .with_example("'S10483'"),
                ],
            ).with_description("All students")],
            ..Default::default()
        };

        let schema = json_schema.to_analyzer_schema();

        // Check that docs are converted to analyzer schema
        let table = schema.get_table("students").unwrap();
        assert!(table.doc.is_some());
        assert_eq!(table.doc.as_ref().unwrap().description, "All students");

        let col_doc = table.column_docs.get("student_id");
        assert!(col_doc.is_some());
        let col_doc = col_doc.unwrap();
        assert_eq!(col_doc.description, "Student ID");
        assert_eq!(col_doc.get_tag("example"), Some("'S10483'"));
    }

    #[test]
    fn test_json_docs_serialization_round_trip() {
        let original = JsonSchema {
            tables: vec![JsonTable {
                name: "t".to_string(),
                columns: vec![JsonColumn {
                    name: "a".to_string(),
                    r#type: None,
                    primary_key: false,
                    not_null: false,
                    description: Some("Column description".to_string()),
                    example: Some("'example'".to_string()),
                    tags: None,
                }],
                without_rowid: false,
                description: Some("Table description".to_string()),
                tags: Some([("details".to_string(), vec!["http://example.com".to_string()])].into()),
            }],
            ..Default::default()
        };

        let json = original.to_json().unwrap();
        let parsed = JsonSchema::from_json(&json).unwrap();

        assert_eq!(original, parsed);
    }
}
