//! Hover information and goto-definition tests

use super::*;

use solite_analyzer::{
    find_statement_at_offset, find_symbol_at_offset, format_hover_content, get_definition_span,
    ResolvedSymbol,
};
use solite_schema::Document;

#[test]
fn test_hover_finds_column() {
    let sql = "SELECT id FROM users";
    let program = parse_program(sql).unwrap();
    let stmt = find_statement_at_offset(&program, 7).unwrap();
    let result = find_symbol_at_offset(stmt, sql, 7, None);

    assert!(result.is_some());
    let (symbol, span) = result.unwrap();
    match symbol {
        ResolvedSymbol::Column { name, .. } => assert_eq!(name, "id"),
        _ => panic!("Expected Column symbol"),
    }
    // Verify span points to "id"
    assert_eq!(&sql[span.start..span.end], "id");
}

#[test]
fn test_hover_qualified_column_resolves_alias() {
    let sql = "SELECT u.id FROM users AS u";
    let program = parse_program(sql).unwrap();
    let stmt = find_statement_at_offset(&program, 9).unwrap();
    let result = find_symbol_at_offset(stmt, sql, 9, None);

    assert!(result.is_some());
    let (symbol, _) = result.unwrap();
    match symbol {
        ResolvedSymbol::Column { name, table_name, qualifier } => {
            assert_eq!(name, "id");
            assert_eq!(table_name, Some("users".to_string()));
            assert_eq!(qualifier, Some("u".to_string()));
        }
        _ => panic!("Expected Column symbol"),
    }
}

#[test]
fn test_hover_table_reference() {
    let sql = "SELECT * FROM users WHERE id = 1";
    let program = parse_program(sql).unwrap();
    let stmt = find_statement_at_offset(&program, 15).unwrap();
    let result = find_symbol_at_offset(stmt, sql, 15, None);

    assert!(result.is_some());
    let (symbol, _) = result.unwrap();
    match symbol {
        ResolvedSymbol::Table { name, .. } => assert_eq!(name, "users"),
        _ => panic!("Expected Table symbol, got {:?}", symbol),
    }
}

#[test]
fn test_hover_content_formatting() {
    let symbol = ResolvedSymbol::TableAlias {
        alias: "u".to_string(),
        table_name: "users".to_string(),
        definition_span: solite_ast::Span::new(0, 1),
    };
    let content = format_hover_content(&symbol, None);
    assert!(content.contains("**u**"));
    assert!(content.contains("alias for `users`"));
}

#[test]
fn test_hover_content_with_schema() {
    let schema = build_test_schema("CREATE TABLE users (id, name, email);");
    let symbol = ResolvedSymbol::Table {
        name: "users".to_string(),
        span: solite_ast::Span::new(0, 5),
    };
    let content = format_hover_content(&symbol, Some(&schema));
    assert!(content.contains("**users**"));
    assert!(content.contains("Columns:"));
    assert!(content.contains("id"));
    assert!(content.contains("name"));
    assert!(content.contains("email"));
}

#[test]
fn test_goto_definition_alias() {
    let sql = "SELECT u.id FROM users AS u WHERE u.name = 'test'";
    let program = parse_program(sql).unwrap();

    // Find the 'u' qualifier in 'u.id' (position 7)
    let stmt = find_statement_at_offset(&program, 7).unwrap();
    let result = find_symbol_at_offset(stmt, sql, 7, None);
    assert!(result.is_some());

    let (symbol, _) = result.unwrap();
    if let ResolvedSymbol::TableAlias { definition_span, .. } = symbol {
        // Definition span should point to WHERE the alias 'u' is defined
        let def_span = get_definition_span(&ResolvedSymbol::TableAlias {
            alias: "u".to_string(),
            table_name: "users".to_string(),
            definition_span: definition_span.clone(),
        });
        assert!(def_span.is_some());
    }
}

#[test]
fn test_goto_definition_returns_none_for_column() {
    // Columns don't have in-document definitions (they're in schema)
    let symbol = ResolvedSymbol::Column {
        name: "id".to_string(),
        table_name: None,
        qualifier: None,
    };
    let def_span = get_definition_span(&symbol);
    assert!(def_span.is_none());
}

#[test]
fn test_hover_column_in_join_on() {
    let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.user_id";
    let program = parse_program(sql).unwrap();

    // Find 'user_id' column (position ~48)
    let stmt = find_statement_at_offset(&program, 48).unwrap();
    let result = find_symbol_at_offset(stmt, sql, 48, None);

    assert!(result.is_some());
    let (symbol, _) = result.unwrap();
    match symbol {
        ResolvedSymbol::Column { name, table_name, qualifier } => {
            assert_eq!(name, "user_id");
            assert_eq!(table_name, Some("orders".to_string()));
            assert_eq!(qualifier, Some("o".to_string()));
        }
        _ => panic!("Expected Column symbol"),
    }
}

#[test]
fn test_doc_comments_in_schema() {
    // Test that doc comments are parsed and attached to schema
    let sql = r#"
CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);
"#;

    let schema = build_test_schema(sql);
    let table_info = schema.get_table("students").expect("students table should exist");

    // Check table doc
    assert!(table_info.doc.is_some(), "Table should have doc comment");
    let table_doc = table_info.doc.as_ref().unwrap();
    assert!(
        table_doc.description.contains("All students at Foo University"),
        "Table doc should contain description, got: {:?}",
        table_doc
    );
    assert!(
        table_doc.tags.contains_key("details"),
        "Table doc should have @details tag"
    );

    // Check column docs
    assert!(
        table_info.column_docs.contains_key("student_id"),
        "student_id should have doc comment"
    );
    let student_id_doc = table_info.column_docs.get("student_id").unwrap();
    assert!(
        student_id_doc.description.contains("Student ID assigned at orientation"),
        "Column doc should contain description"
    );
    assert!(
        student_id_doc.tags.contains_key("example"),
        "Column doc should have @example tag"
    );
}

#[test]
fn test_hover_with_doc_comments() {
    // Test that doc comments appear in hover content - exact format from user report
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);

select * from students where student_id = 3;
"#;

    let schema = build_test_schema(sql);
    let symbol = ResolvedSymbol::Table {
        name: "students".to_string(),
        span: solite_ast::Span::new(0, 8),
    };
    let content = format_hover_content(&symbol, Some(&schema));

    // Verify doc content appears in hover
    assert!(
        content.contains("All students at Foo University"),
        "Hover should contain table description, got: {}",
        content
    );
    assert!(
        content.contains("https://foo.edu/students"),
        "Hover should contain @details value, got: {}",
        content
    );
    assert!(
        content.contains("Student ID assigned at orientation"),
        "Hover should contain column description, got: {}",
        content
    );
}

#[test]
fn test_hover_on_table_in_select_with_doc_comments() {
    // Test hovering on table name in SELECT statement shows docs from CREATE TABLE
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);

select * from students where student_id = 3;
"#;

    let program = parse_program(sql).unwrap();
    let schema = build_test_schema(sql);

    // Find "students" in the SELECT statement (around position 220)
    // The select starts after the CREATE TABLE statement
    let select_start = sql.find("select").unwrap();
    let students_in_select = sql[select_start..].find("students").unwrap() + select_start;

    // Find symbol at that position
    let stmt = find_statement_at_offset(&program, students_in_select).unwrap();
    let result = find_symbol_at_offset(stmt, sql, students_in_select, Some(&schema));

    assert!(result.is_some(), "Should find symbol at students position");
    let (symbol, _) = result.unwrap();

    // Verify it's a Table symbol
    match &symbol {
        ResolvedSymbol::Table { name, .. } => {
            assert_eq!(name, "students");
        }
        _ => panic!("Expected Table symbol, got {:?}", symbol),
    }

    // Get hover content
    let content = format_hover_content(&symbol, Some(&schema));

    // Print the actual hover content for debugging
    println!("\n=== Table hover content ===\n{}\n=== End ===\n", content);

    // Verify doc content appears
    assert!(
        content.contains("All students at Foo University"),
        "Hover should contain table description, got: {}",
        content
    );
}

#[test]
fn test_hover_column_with_doc_comments() {
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);"#;

    let schema = build_test_schema(sql);

    // Column hover
    let col_symbol = ResolvedSymbol::Column {
        name: "student_id".to_string(),
        table_name: Some("students".to_string()),
        qualifier: None,
    };
    let col_content = format_hover_content(&col_symbol, Some(&schema));

    println!("\n=== Column hover content ===\n{}\n=== End ===\n", col_content);

    assert!(
        col_content.contains("Student ID assigned at orientation"),
        "Column hover should contain description, got: {}",
        col_content
    );
    assert!(
        col_content.contains("'S10483'"),
        "Column hover should contain @example value, got: {}",
        col_content
    );
}

/// This test simulates the EXACT flow that happens in VS Code:
/// 1. Document::parse() is called (like in on_change/did_open)
/// 2. build_schema() is called on the parsed program
/// 3. Hover uses parse_program() again to get the AST
/// 4. format_hover_content() is called with the schema from step 2
#[test]
fn test_lsp_flow_doc_comments() {
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);

select * from students where student_id = 3;
"#;

    // Step 1: Document::parse (what on_change does)
    let doc = Document::parse(sql, true);
    println!("Document parsed. Dot commands: {:?}", doc.dot_commands);
    println!("SQL regions: {:?}", doc.sql_regions);

    // Step 2: Check if the program parsed successfully
    let program = match &doc.program {
        Ok(p) => {
            println!("Program parsed successfully with {} statements", p.statements.len());
            p
        }
        Err(errors) => {
            panic!("Parse errors: {:?}", errors);
        }
    };

    // Step 3: Check the CREATE TABLE statement for doc comments
    if let solite_ast::Statement::CreateTable(create) = &program.statements[0] {
        println!("CREATE TABLE {} found", create.table_name);
        println!("  Table doc: {:?}", create.doc);
        for col in &create.columns {
            println!("  Column {}: doc = {:?}", col.name, col.doc);
        }
    }

    // Step 4: Build schema (what on_change does)
    let schema = build_schema(program);

    // Step 5: Verify schema has the docs
    let table_info = schema.get_table("students").expect("students table should exist in schema");
    println!("\nSchema table info for 'students':");
    println!("  Table doc: {:?}", table_info.doc);
    println!("  Column docs: {:?}", table_info.column_docs);

    assert!(
        table_info.doc.is_some(),
        "Schema should have table doc"
    );
    assert!(
        table_info.column_docs.contains_key("student_id"),
        "Schema should have column doc for student_id"
    );

    // Step 6: Simulate hover - parse again (what hover() does)
    let hover_program = parse_program(sql).expect("Hover parse should succeed");

    // Step 7: Find the table in SELECT statement
    let select_start = sql.find("select").unwrap();
    let students_pos = sql[select_start..].find("students").unwrap() + select_start;
    println!("\nLooking for symbol at position {} (in SELECT statement)", students_pos);

    let stmt = find_statement_at_offset(&hover_program, students_pos)
        .expect("Should find statement at offset");
    let (symbol, span) = find_symbol_at_offset(stmt, sql, students_pos, Some(&schema))
        .expect("Should find symbol at offset");

    println!("Found symbol: {:?}", symbol);
    println!("Symbol span: {:?} = '{}'", span, &sql[span.start..span.end]);

    // Step 8: Format hover content (what hover() does)
    let content = format_hover_content(&symbol, Some(&schema));
    println!("\n=== HOVER CONTENT (what VS Code should show) ===\n{}\n=== END ===", content);

    // Verify the doc appears
    assert!(
        content.contains("All students at Foo University"),
        "Hover content should contain table doc. Got:\n{}",
        content
    );
}

/// Diagnostic test - this prints exactly what VS Code should receive
/// Run with: cargo test -p solite_lsp test_vscode_hover_diagnostic -- --nocapture
#[test]
fn test_vscode_hover_diagnostic() {
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);

select * from students where student_id = 3;
"#;

    println!("\n==========================================================");
    println!("DIAGNOSTIC: Simulating VS Code hover on 'students' in SELECT");
    println!("==========================================================\n");

    // Step 1: Document::parse (what on_change does when file is opened)
    let doc = Document::parse(sql, true);
    println!("1. Document parsed (enable_dot_commands=true)");

    // Step 2: Build schema (stored in self.schemas during on_change)
    let schema = match &doc.program {
        Ok(program) => {
            let s = build_schema(program);
            println!("2. Schema built from {} statements", program.statements.len());
            s
        }
        Err(e) => panic!("Parse error: {:?}", e),
    };

    // Step 3: Check schema has docs
    if let Some(table) = schema.get_table("students") {
        println!("3. Schema has 'students' table:");
        println!("   - Table doc: {}", if table.doc.is_some() { "YES" } else { "NO" });
        println!("   - Column docs: {:?}", table.column_docs.keys().collect::<Vec<_>>());
    } else {
        println!("3. ERROR: Schema does NOT have 'students' table!");
    }

    // Step 4: Simulate hover - parse the text again (what hover() does)
    let hover_program = parse_program(sql).expect("Hover parse should work");
    println!("4. Hover re-parsed text: {} statements", hover_program.statements.len());

    // Step 5: Find position of 'students' in SELECT
    let select_pos = sql.find("select").unwrap();
    let students_pos = sql[select_pos..].find("students").unwrap() + select_pos;
    println!("5. Looking for symbol at position {} ('{}')",
             students_pos,
             &sql[students_pos..students_pos+8]);

    // Step 6: Find statement and symbol
    let stmt = find_statement_at_offset(&hover_program, students_pos);
    if stmt.is_none() {
        println!("   ERROR: No statement found at offset!");
        return;
    }
    println!("   Found statement at offset");

    let result = find_symbol_at_offset(stmt.unwrap(), sql, students_pos, Some(&schema));
    if result.is_none() {
        println!("   ERROR: No symbol found at offset!");
        return;
    }
    let (symbol, span) = result.unwrap();
    println!("   Found symbol: {:?}", symbol);
    println!("   Span: {} = '{}'", span.start, &sql[span.start..span.end]);

    // Step 7: Format hover content
    let content = format_hover_content(&symbol, Some(&schema));

    println!("\n==========================================================");
    println!("HOVER CONTENT (what VS Code should display):");
    println!("==========================================================");
    println!("{}", content);
    println!("==========================================================\n");

    // Verify
    assert!(content.contains("All students at Foo University"),
            "Missing table doc!");
    assert!(content.contains("Student ID assigned at orientation"),
            "Missing column doc!");

    println!("✓ All checks passed - hover content is correct");
    println!("\nIf VS Code doesn't show this, try:");
    println!("  1. Rebuild: cargo build -p solite_lsp --release");
    println!("  2. In VS Code: Cmd+Shift+P → 'Developer: Reload Window'");
    println!("  3. Verify extension uses the correct LSP binary path");
}

/// Test that shows the SQL region splitting issue
#[test]
fn test_sql_region_splitting_with_doc_comments() {
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);

select * from students where student_id = 3;
"#;

    // This is what Document::parse does
    let result = solite_schema::dotcmd::parse_dot_commands(sql);
    println!("SQL regions: {:?}", result.sql_regions);

    // Show what each region contains
    for (i, region) in result.sql_regions.iter().enumerate() {
        let content = &sql[region.start..region.end];
        println!("\nRegion {} ({}-{}):\n---\n{}\n---", i, region.start, region.end, content);
    }

    // The SQL regions are joined with \n
    let joined_sql: String = result
        .sql_regions
        .iter()
        .map(|r| &sql[r.start..r.end])
        .collect::<Vec<_>>()
        .join("\n");

    println!("\n=== JOINED SQL ===\n{}\n=== END ===", joined_sql);

    // Parse the joined SQL
    let program = parse_program(&joined_sql).expect("Should parse");
    println!("\nParsed {} statements", program.statements.len());

    // Check for doc comments
    if let solite_ast::Statement::CreateTable(create) = &program.statements[0] {
        println!("Table doc: {:?}", create.doc);
        for col in &create.columns {
            println!("Column {}: {:?}", col.name, col.doc);
        }
    }
}
