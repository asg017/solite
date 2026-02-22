//! SQL constants for documentation generation.

/// Create table to store base functions (before extension load).
pub const BASE_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_base_functions AS
    SELECT name
    FROM pragma_function_list
    ORDER BY 1
"#;

/// Create table to store base modules (before extension load).
pub const BASE_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_base_modules AS
    SELECT name
    FROM pragma_module_list
    ORDER BY 1
"#;

/// Create table to store loaded functions (from extension).
pub const LOADED_FUNCTIONS_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_loaded_functions AS
    SELECT name
    FROM pragma_function_list
    WHERE name NOT IN (SELECT name FROM solite_docs.solite_docs_base_functions)
    ORDER BY 1
"#;

/// Create table to store loaded modules (from extension).
pub const LOADED_MODULES_CREATE: &str = r#"
  CREATE TABLE solite_docs.solite_docs_loaded_modules AS
    SELECT name
    FROM pragma_module_list
    WHERE name NOT IN (SELECT name FROM solite_docs.solite_docs_base_modules)
    ORDER BY 1
"#;
