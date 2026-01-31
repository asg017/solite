use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::completions::{
    get_completions_extended, CompletionOptions as ExtendedCompletionOptions,
};
use crate::context::detect_context;
use solite_schema::{Document, DotCommand, FileSchemaProvider, SchemaProvider, SqlRegion};
use solite_analyzer::{
    analyze_with_schema, build_schema, find_statement_at_offset, find_symbol_at_offset,
    format_hover_content, get_definition_span, lint_with_config, Diagnostic, LintConfig,
    LintDiagnostic, LintResult, RuleSeverity, Schema, Severity,
};
use solite_ast::Program;
use solite_ast::Span;
use solite_lexer::{lex, TokenKind};
use solite_fmt::{FormatConfig, IndentStyle, format_document};
use solite_parser::parse_program;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

// Semantic token types - order matters, index is used in the protocol
const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,  // 0
    SemanticTokenType::VARIABLE, // 1
    SemanticTokenType::NUMBER,   // 2
    SemanticTokenType::STRING,   // 3
    SemanticTokenType::COMMENT,  // 4
    SemanticTokenType::OPERATOR, // 5
    SemanticTokenType::TYPE,     // 6
];

/// Context for tracking type positions during semantic highlighting
#[derive(Debug, Clone, Copy, PartialEq)]
enum TypeContext {
    /// Normal context - no special highlighting
    Normal,
    /// After CREATE TABLE name - waiting for (
    AfterCreateTable,
    /// Inside CREATE TABLE column list - expecting column name
    ExpectColumnName,
    /// After column name - expecting type or constraint
    ExpectColumnType,
    /// Inside type with parens - e.g., VARCHAR(
    InsideTypeParen,
    /// After CAST( - waiting for AS
    InCastExpr,
    /// After CAST(... AS - expecting type
    ExpectCastType,
    /// After ALTER TABLE name ADD [COLUMN] - expecting column name
    ExpectAlterColumnName,
    /// After ALTER TABLE ADD column_name - expecting type
    ExpectAlterColumnType,
    /// Inside GENERATED ALWAYS AS (...) or AS (...) expression
    /// The i32 tracks the paren depth when we entered
    InGeneratedExpr(i32),
    /// Inside CHECK(...) or DEFAULT(...) expression
    /// The i32 tracks the paren depth when we entered
    InConstraintExpr(i32),
}

/// Update the type context based on the current token
fn update_type_context(
    current: TypeContext,
    kind: &TokenKind,
    tokens: &[solite_lexer::Token],
    index: usize,
    paren_depth: &mut i32,
    cast_paren_depth: &mut i32,
) -> TypeContext {
    // Track parentheses
    match kind {
        TokenKind::LParen => *paren_depth += 1,
        TokenKind::RParen => *paren_depth = (*paren_depth - 1).max(0),
        _ => {}
    }

    match current {
        TypeContext::Normal => {
            match kind {
                TokenKind::Create => {
                    if let Some(next) = tokens.get(index + 1) {
                        if next.kind == TokenKind::Table {
                            return TypeContext::AfterCreateTable;
                        }
                    }
                }
                TokenKind::Cast => {
                    *cast_paren_depth = *paren_depth;
                    return TypeContext::InCastExpr;
                }
                TokenKind::Add => {
                    for j in (0..index).rev() {
                        match tokens[j].kind {
                            TokenKind::Alter => return TypeContext::ExpectAlterColumnName,
                            TokenKind::Semicolon | TokenKind::Create | TokenKind::Drop => break,
                            _ => continue,
                        }
                    }
                }
                _ => {}
            }
            TypeContext::Normal
        }

        TypeContext::AfterCreateTable => {
            match kind {
                TokenKind::Table => TypeContext::AfterCreateTable,
                TokenKind::If | TokenKind::Not | TokenKind::Exists => TypeContext::AfterCreateTable,
                // table name (including quoted identifiers)
                TokenKind::Ident
                | TokenKind::QuotedIdent
                | TokenKind::BracketIdent
                | TokenKind::BacktickIdent => TypeContext::AfterCreateTable,
                TokenKind::LParen => TypeContext::ExpectColumnName,
                _ => TypeContext::Normal,
            }
        }

        TypeContext::ExpectColumnName => {
            match kind {
                // column name (including quoted identifiers)
                TokenKind::Ident
                | TokenKind::QuotedIdent
                | TokenKind::BracketIdent
                | TokenKind::BacktickIdent => TypeContext::ExpectColumnType,
                TokenKind::RParen => TypeContext::Normal,
                TokenKind::Comma => TypeContext::ExpectColumnName,
                TokenKind::Primary | TokenKind::Unique | TokenKind::Check
                | TokenKind::Foreign | TokenKind::Constraint => TypeContext::ExpectColumnName,
                _ => TypeContext::ExpectColumnName,
            }
        }

        TypeContext::ExpectColumnType => {
            match kind {
                TokenKind::Ident => TypeContext::ExpectColumnType,
                TokenKind::LParen => TypeContext::InsideTypeParen,
                TokenKind::Comma => TypeContext::ExpectColumnName,
                TokenKind::RParen => TypeContext::Normal,
                // GENERATED ALWAYS AS (...) - enter generated expression mode
                TokenKind::Generated => TypeContext::InGeneratedExpr(*paren_depth),
                // AS (...) shorthand for generated columns
                TokenKind::As => {
                    // Check if next token is LParen (generated column shorthand)
                    if let Some(next) = tokens.get(index + 1) {
                        if next.kind == TokenKind::LParen {
                            return TypeContext::InGeneratedExpr(*paren_depth);
                        }
                    }
                    TypeContext::ExpectColumnName
                }
                // CHECK(...) and DEFAULT(...) have expressions - don't color identifiers as types
                TokenKind::Check => TypeContext::InConstraintExpr(*paren_depth),
                TokenKind::Default => {
                    // Check if next token is LParen (expression default)
                    if let Some(next) = tokens.get(index + 1) {
                        if next.kind == TokenKind::LParen {
                            return TypeContext::InConstraintExpr(*paren_depth);
                        }
                    }
                    // Literal default - stay in column context
                    TypeContext::ExpectColumnName
                }
                TokenKind::Primary | TokenKind::Not | TokenKind::Null
                | TokenKind::Unique | TokenKind::Collate | TokenKind::References
                | TokenKind::Constraint | TokenKind::Autoincrement => TypeContext::ExpectColumnName,
                _ => TypeContext::ExpectColumnType,
            }
        }

        TypeContext::InsideTypeParen => {
            match kind {
                TokenKind::RParen => TypeContext::ExpectColumnType,
                _ => TypeContext::InsideTypeParen,
            }
        }

        TypeContext::InGeneratedExpr(entry_depth) => {
            // Stay in generated expr until we close back to entry depth
            match kind {
                TokenKind::RParen if *paren_depth == entry_depth => {
                    TypeContext::ExpectColumnName
                }
                _ => TypeContext::InGeneratedExpr(entry_depth),
            }
        }

        TypeContext::InConstraintExpr(entry_depth) => {
            // Stay in constraint expr until we close back to entry depth
            match kind {
                TokenKind::RParen if *paren_depth == entry_depth => {
                    TypeContext::ExpectColumnName
                }
                _ => TypeContext::InConstraintExpr(entry_depth),
            }
        }

        TypeContext::InCastExpr => {
            match kind {
                TokenKind::As => TypeContext::ExpectCastType,
                TokenKind::RParen if *paren_depth < *cast_paren_depth => TypeContext::Normal,
                _ => TypeContext::InCastExpr,
            }
        }

        TypeContext::ExpectCastType => {
            match kind {
                TokenKind::Ident => TypeContext::ExpectCastType,
                TokenKind::LParen => TypeContext::InsideTypeParen,
                TokenKind::RParen => TypeContext::Normal,
                _ => TypeContext::Normal,
            }
        }

        TypeContext::ExpectAlterColumnName => {
            match kind {
                TokenKind::Column => TypeContext::ExpectAlterColumnName,
                // column name (including quoted identifiers)
                TokenKind::Ident
                | TokenKind::QuotedIdent
                | TokenKind::BracketIdent
                | TokenKind::BacktickIdent => TypeContext::ExpectAlterColumnType,
                _ => TypeContext::Normal,
            }
        }

        TypeContext::ExpectAlterColumnType => {
            match kind {
                TokenKind::Ident => TypeContext::ExpectAlterColumnType,
                TokenKind::LParen => TypeContext::InsideTypeParen,
                TokenKind::Semicolon | TokenKind::Comma => TypeContext::Normal,
                TokenKind::Primary | TokenKind::Not | TokenKind::Null
                | TokenKind::Unique | TokenKind::Check | TokenKind::Default
                | TokenKind::Collate | TokenKind::References | TokenKind::Generated
                | TokenKind::Constraint => TypeContext::Normal,
                _ => TypeContext::ExpectAlterColumnType,
            }
        }
    }
}

const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[];

/// Extract the notebook path from a notebook cell URI.
/// Cell URIs look like: vscode-notebook-cell:/path/to/notebook.ipynb#W0sZmlsZQ...
/// Returns None if this is not a notebook cell URI.
fn get_notebook_path(uri: &Url) -> Option<String> {
    if uri.scheme() == "vscode-notebook-cell" {
        // The path portion contains the notebook file path
        Some(uri.path().to_string())
    } else {
        None
    }
}

/// Build a combined schema from multiple SQL source texts.
fn build_combined_schema(sources: &[&str]) -> Schema {
    // Extract SQL-only content from each source (filter out dot commands)
    let sql_sources: Vec<String> = sources
        .iter()
        .map(|source| {
            let doc = Document::parse(source, true);
            doc.sql_regions
                .iter()
                .map(|r| &source[r.start..r.end])
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();

    // Concatenate all SQL sources with semicolons to ensure statement separation
    let combined = sql_sources.join(";\n");
    if let Ok(program) = parse_program(&combined) {
        build_schema(&program)
    } else {
        // If combined parsing fails, try each source individually and merge
        let mut combined_program = Program { statements: vec![] };
        for source in &sql_sources {
            if let Ok(program) = parse_program(source) {
                combined_program.statements.extend(program.statements);
            }
        }
        build_schema(&combined_program)
    }
}

pub(crate) struct Backend {
    client: Client,
    documents: RwLock<HashMap<Url, String>>,
    schemas: RwLock<HashMap<Url, Schema>>,
    /// Tracks notebook cell contents: notebook_path -> (cell_uri -> cell_content)
    notebook_cells: RwLock<HashMap<String, HashMap<Url, String>>>,
    /// Combined schemas for notebooks: notebook_path -> Schema (from DDL in cells)
    notebook_schemas: RwLock<HashMap<String, Schema>>,
    /// External schemas from .open commands in notebooks: notebook_path -> Schema
    notebook_open_schemas: RwLock<HashMap<String, Schema>>,
    /// Lint results with fixes for each document
    lint_results: RwLock<HashMap<Url, Vec<LintResult>>>,
    /// External schemas from .open commands per document (regular files)
    open_schemas: RwLock<HashMap<Url, Schema>>,
}

impl Backend {
    pub(crate) fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
            schemas: RwLock::new(HashMap::new()),
            notebook_cells: RwLock::new(HashMap::new()),
            notebook_schemas: RwLock::new(HashMap::new()),
            notebook_open_schemas: RwLock::new(HashMap::new()),
            lint_results: RwLock::new(HashMap::new()),
            open_schemas: RwLock::new(HashMap::new()),
        }
    }

    async fn on_change(&self, uri: Url, text: String) {
        // Check if this is a notebook cell
        if let Some(notebook_path) = get_notebook_path(&uri) {
            // Store cell content
            {
                let mut notebook_cells = self
                    .notebook_cells
                    .write()
                    .expect("notebook_cells lock poisoned");
                let cells = notebook_cells.entry(notebook_path.clone()).or_default();
                cells.insert(uri.clone(), text.clone());
            }

            // Store in documents
            self.documents
                .write()
                .expect("documents lock poisoned")
                .insert(uri.clone(), text);

            // Get base path for resolving relative .open paths (notebook directory)
            let base_path = PathBuf::from(&notebook_path)
                .parent()
                .map(|p| p.to_path_buf());

            // Build combined schema from all cells and process .open commands
            {
                let notebook_cells = self
                    .notebook_cells
                    .read()
                    .expect("notebook_cells lock poisoned");
                if let Some(cells) = notebook_cells.get(&notebook_path) {
                    // Build DDL schema from SQL in all cells
                    let sources: Vec<&str> = cells.values().map(|s| s.as_str()).collect();
                    let combined_schema = build_combined_schema(&sources);
                    self.notebook_schemas
                        .write()
                        .expect("notebook_schemas lock poisoned")
                        .insert(notebook_path.clone(), combined_schema);

                    // Process .open commands from all cells to build external schema
                    let mut external_schema = Schema::new();
                    for cell_content in cells.values() {
                        let doc = Document::parse(cell_content, true);
                        for cmd in &doc.dot_commands {
                            match cmd {
                                DotCommand::Open { path, .. } => {
                                    let db_path = if let Some(ref base) = base_path {
                                        let path_buf = PathBuf::from(path);
                                        if path_buf.is_absolute() {
                                            path_buf
                                        } else {
                                            base.join(path)
                                        }
                                    } else {
                                        PathBuf::from(path)
                                    };

                                    let provider = FileSchemaProvider::new(&db_path);
                                    if let Ok(schema) = provider.load() {
                                        external_schema.merge(schema);
                                    }
                                }
                            }
                        }
                    }
                    self.notebook_open_schemas
                        .write()
                        .expect("notebook_open_schemas lock poisoned")
                        .insert(notebook_path.clone(), external_schema);
                }
            }

            // Re-publish diagnostics for ALL cells in this notebook
            let cell_uris: Vec<Url> = {
                let notebook_cells = self
                    .notebook_cells
                    .read()
                    .expect("notebook_cells lock poisoned");
                notebook_cells
                    .get(&notebook_path)
                    .map(|cells| cells.keys().cloned().collect())
                    .unwrap_or_default()
            };

            for cell_uri in cell_uris {
                let cell_text = {
                    let documents = self.documents.read().expect("documents lock poisoned");
                    documents.get(&cell_uri).cloned()
                };
                if let Some(cell_text) = cell_text {
                    let diagnostics = self.compute_diagnostics_for_uri(&cell_uri, &cell_text);
                    self.client
                        .publish_diagnostics(cell_uri, diagnostics, None)
                        .await;
                }
            }
        } else {
            // Regular file - parse with dot commands enabled
            let doc = Document::parse(&text, true);

            // Process .open commands and build external schema
            let mut external_schema = Schema::new();
            let mut open_diagnostics: Vec<tower_lsp::lsp_types::Diagnostic> = Vec::new();

            // Get the base path for resolving relative paths
            let base_path = uri
                .to_file_path()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));

            for cmd in &doc.dot_commands {
                match cmd {
                    DotCommand::Open { path, span } => {
                        // Resolve path relative to document
                        let db_path = if let Some(ref base) = base_path {
                            let path_buf = PathBuf::from(path);
                            if path_buf.is_absolute() {
                                path_buf
                            } else {
                                base.join(path)
                            }
                        } else {
                            PathBuf::from(path)
                        };

                        // Try to load schema from the database
                        let provider = FileSchemaProvider::new(&db_path);
                        match provider.load() {
                            Ok(introspected_schema) => {
                                external_schema.merge(introspected_schema);
                            }
                            Err(e) => {
                                // Add diagnostic for failed .open
                                let range = span_to_range(&text, span);
                                open_diagnostics.push(tower_lsp::lsp_types::Diagnostic {
                                    range,
                                    severity: Some(DiagnosticSeverity::WARNING),
                                    message: format!("Failed to open database: {}", e),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }

            // Store external schema from .open commands
            self.open_schemas
                .write()
                .expect("open_schemas lock poisoned")
                .insert(uri.clone(), external_schema);

            // Build schema from this document's DDL
            if let Ok(ref program) = doc.program {
                let schema = build_schema(program);
                self.schemas
                    .write()
                    .expect("schemas lock poisoned")
                    .insert(uri.clone(), schema);
            }

            // Get external schema for this file
            let external_schema: Option<Schema> = self
                .open_schemas
                .read()
                .expect("open_schemas lock poisoned")
                .get(&uri)
                .cloned();

            // Compute diagnostics using the pre-parsed document (respects dot commands)
            let (mut diagnostics, lint_results) =
                self.compute_diagnostics_for_document(&doc, external_schema.as_ref());

            // Store lint results for code actions
            self.lint_results
                .write()
                .expect("lint_results lock poisoned")
                .insert(uri.clone(), lint_results);

            // Prepend .open error diagnostics
            diagnostics.splice(0..0, open_diagnostics);

            self.documents
                .write()
                .expect("documents lock poisoned")
                .insert(uri.clone(), text);
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    /// Compute diagnostics, using notebook schema or open_schema for cross-cell/file validation
    fn compute_diagnostics_for_uri(&self, uri: &Url, text: &str) -> Vec<tower_lsp::lsp_types::Diagnostic> {
        // Get the appropriate external schema based on document type
        let notebook_path = get_notebook_path(uri);

        let external_schema: Option<Schema> = if let Some(ref nb_path) = notebook_path {
            // For notebook cells, combine DDL schema with .open schema
            let ddl_schema = self.notebook_schemas
                .read()
                .expect("notebook_schemas lock poisoned")
                .get(nb_path)
                .cloned();
            let open_schema = self.notebook_open_schemas
                .read()
                .expect("notebook_open_schemas lock poisoned")
                .get(nb_path)
                .cloned();

            // Merge: open schema provides external tables, DDL schema provides local tables
            match (ddl_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        } else {
            // For regular files, use the schema from .open commands
            self.open_schemas
                .read()
                .expect("open_schemas lock poisoned")
                .get(uri)
                .cloned()
        };

        // Parse document with dot commands to filter out .open lines
        let doc = Document::parse(text, true);

        let (diagnostics, lint_results) = self.compute_diagnostics_for_document(&doc, external_schema.as_ref());

        // Store lint results for code actions
        self.lint_results
            .write()
            .expect("lint_results lock poisoned")
            .insert(uri.clone(), lint_results);

        diagnostics
    }

    fn compute_semantic_tokens(&self, text: &str) -> Vec<SemanticToken> {
        compute_semantic_tokens(text)
    }

    /// Compute diagnostics for a Document that may have dot commands.
    /// Spans are mapped from the joined SQL text back to the original source.
    fn compute_diagnostics_for_document(
        &self,
        doc: &Document,
        external_schema: Option<&Schema>,
    ) -> (Vec<tower_lsp::lsp_types::Diagnostic>, Vec<LintResult>) {
        // Build the joined SQL text for lint/analysis (which need the source text)
        let sql_source: String = doc
            .sql_regions
            .iter()
            .map(|r| &doc.source[r.start..r.end])
            .collect::<Vec<_>>()
            .join("\n");

        let mut lsp_diagnostics = Vec::new();
        let mut all_lint_results = Vec::new();

        match &doc.program {
            Ok(program) => {
                // Load lint config (discovers solite-lint.toml)
                let config = LintConfig::discover();

                // Run lint system with config and external schema
                let lint_results = lint_with_config(program, &sql_source, &config, external_schema);
                for result in &lint_results {
                    // Map span from SQL text back to original source
                    let mapped_span = map_span_to_source(&result.diagnostic.span, &doc.sql_regions);
                    let range = span_to_range(&doc.source, &mapped_span);
                    let severity = match result.diagnostic.severity {
                        RuleSeverity::Error => DiagnosticSeverity::ERROR,
                        RuleSeverity::Warning => DiagnosticSeverity::WARNING,
                        RuleSeverity::Off => DiagnosticSeverity::HINT,
                    };
                    lsp_diagnostics.push(tower_lsp::lsp_types::Diagnostic {
                        range,
                        severity: Some(severity),
                        code: Some(NumberOrString::String(result.diagnostic.rule_id.to_string())),
                        message: result.diagnostic.message.clone(),
                        ..Default::default()
                    });
                }
                all_lint_results = lint_results;

                // Also run semantic analysis for non-lint diagnostics
                let analyzer_diagnostics = analyze_with_schema(program, external_schema);
                for diag in analyzer_diagnostics {
                    let mapped_span = map_span_to_source(&diag.span, &doc.sql_regions);
                    let range = span_to_range(&doc.source, &mapped_span);
                    let severity = match diag.severity {
                        Severity::Error => DiagnosticSeverity::ERROR,
                        Severity::Warning => DiagnosticSeverity::WARNING,
                    };
                    lsp_diagnostics.push(tower_lsp::lsp_types::Diagnostic {
                        range,
                        severity: Some(severity),
                        message: diag.message.clone(),
                        ..Default::default()
                    });
                }
            }
            Err(parse_errors) => {
                // Convert parse errors to diagnostics, mapping spans
                for err in parse_errors {
                    let position = err.position();
                    let mapped_position = map_offset_to_source(position, &doc.sql_regions);
                    let (line, character) = offset_to_position(&doc.source, mapped_position);
                    lsp_diagnostics.push(tower_lsp::lsp_types::Diagnostic {
                        range: Range {
                            start: Position { line, character },
                            end: Position {
                                line,
                                character: character + 1,
                            },
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: err.to_string(),
                        ..Default::default()
                    });
                }
            }
        }

        (lsp_diagnostics, all_lint_results)
    }
}

/// Compute semantic tokens for SQL text (standalone function for testing)
pub(crate) fn compute_semantic_tokens(text: &str) -> Vec<SemanticToken> {
    let tokens = lex(text);
    let mut semantic_tokens = Vec::new();
        let mut prev_line = 0u32;
        let mut prev_start = 0u32;
        let mut type_context = TypeContext::Normal;
        let mut paren_depth = 0i32;
        let mut cast_paren_depth = 0i32;

        for (i, token) in tokens.iter().enumerate() {
            // Check if this identifier is in a type position
            let is_type_position = matches!(
                type_context,
                TypeContext::ExpectColumnType
                    | TypeContext::ExpectCastType
                    | TypeContext::ExpectAlterColumnType
                    | TypeContext::InsideTypeParen
            ) && token.kind == TokenKind::Ident;

            let token_type = if is_type_position {
                6 // type
            } else {
                match token.kind {
                    TokenKind::Comment | TokenKind::BlockComment => 4, // comment
                    // All SQL keywords
                    TokenKind::Select
                    | TokenKind::Insert
                    | TokenKind::Update
                    | TokenKind::Delete
                    | TokenKind::Replace
                    | TokenKind::Into
                    | TokenKind::Values
                    | TokenKind::Set
                    | TokenKind::From
                    | TokenKind::Create
                    | TokenKind::Drop
                    | TokenKind::Alter
                    | TokenKind::Table
                    | TokenKind::Index
                    | TokenKind::View
                    | TokenKind::Trigger
                    | TokenKind::Virtual
                    | TokenKind::Temp
                    | TokenKind::Temporary
                    | TokenKind::If
                    | TokenKind::Add
                    | TokenKind::Column
                    | TokenKind::Rename
                    | TokenKind::Begin
                    | TokenKind::Commit
                    | TokenKind::Rollback
                    | TokenKind::Savepoint
                    | TokenKind::Release
                    | TokenKind::Transaction
                    | TokenKind::Deferred
                    | TokenKind::Immediate
                    | TokenKind::Exclusive
                    | TokenKind::End
                    | TokenKind::Where
                    | TokenKind::Order
                    | TokenKind::By
                    | TokenKind::Group
                    | TokenKind::Having
                    | TokenKind::Limit
                    | TokenKind::Offset
                    | TokenKind::Distinct
                    | TokenKind::All
                    | TokenKind::As
                    | TokenKind::Asc
                    | TokenKind::Desc
                    | TokenKind::Nulls
                    | TokenKind::First
                    | TokenKind::Last
                    | TokenKind::Union
                    | TokenKind::Intersect
                    | TokenKind::Except
                    | TokenKind::Indexed
                    | TokenKind::Join
                    | TokenKind::Inner
                    | TokenKind::Left
                    | TokenKind::Right
                    | TokenKind::Full
                    | TokenKind::Outer
                    | TokenKind::Cross
                    | TokenKind::Natural
                    | TokenKind::On
                    | TokenKind::Using
                    | TokenKind::And
                    | TokenKind::Or
                    | TokenKind::Not
                    | TokenKind::In
                    | TokenKind::Between
                    | TokenKind::Like
                    | TokenKind::Glob
                    | TokenKind::Regexp
                    | TokenKind::Match
                    | TokenKind::Escape
                    | TokenKind::Is
                    | TokenKind::IsNull
                    | TokenKind::NotNull
                    | TokenKind::Exists
                    | TokenKind::Null
                    | TokenKind::True
                    | TokenKind::False
                    | TokenKind::CurrentDate
                    | TokenKind::CurrentTime
                    | TokenKind::CurrentTimestamp
                    | TokenKind::Case
                    | TokenKind::When
                    | TokenKind::Then
                    | TokenKind::Else
                    | TokenKind::Cast
                    | TokenKind::Constraint
                    | TokenKind::Primary
                    | TokenKind::Key
                    | TokenKind::Unique
                    | TokenKind::Check
                    | TokenKind::Default
                    | TokenKind::Collate
                    | TokenKind::Foreign
                    | TokenKind::References
                    | TokenKind::Autoincrement
                    | TokenKind::Cascade
                    | TokenKind::Restrict
                    | TokenKind::No
                    | TokenKind::Action
                    | TokenKind::Deferrable
                    | TokenKind::Initially
                    | TokenKind::Before
                    | TokenKind::After
                    | TokenKind::Instead
                    | TokenKind::Of
                    | TokenKind::For
                    | TokenKind::Each
                    | TokenKind::Row
                    | TokenKind::Raise
                    | TokenKind::Over
                    | TokenKind::Partition
                    | TokenKind::Window
                    | TokenKind::Rows
                    | TokenKind::Range
                    | TokenKind::Groups
                    | TokenKind::Unbounded
                    | TokenKind::Preceding
                    | TokenKind::Following
                    | TokenKind::Current
                    | TokenKind::Filter
                    | TokenKind::Exclude
                    | TokenKind::Ties
                    | TokenKind::Others
                    | TokenKind::With
                    | TokenKind::Recursive
                    | TokenKind::Materialized
                    | TokenKind::Abort
                    | TokenKind::Fail
                    | TokenKind::Ignore
                    | TokenKind::Conflict
                    | TokenKind::Do
                    | TokenKind::Nothing
                    | TokenKind::Generated
                    | TokenKind::Always
                    | TokenKind::Stored
                    | TokenKind::Explain
                    | TokenKind::Query
                    | TokenKind::Plan
                    | TokenKind::Pragma
                    | TokenKind::Analyze
                    | TokenKind::Attach
                    | TokenKind::Detach
                    | TokenKind::Database
                    | TokenKind::Vacuum
                    | TokenKind::Reindex
                    | TokenKind::Returning
                    | TokenKind::Without
                    | TokenKind::To
                    | TokenKind::Within => 0, // keyword
                    TokenKind::Ident
                    | TokenKind::QuotedIdent
                    | TokenKind::BracketIdent
                    | TokenKind::BacktickIdent
                    | TokenKind::BindParam
                    | TokenKind::BindParamColon
                    | TokenKind::BindParamAt
                    | TokenKind::BindParamDollar => 1, // variable
                    TokenKind::Integer | TokenKind::Float | TokenKind::HexInteger => 2, // number
                    TokenKind::String | TokenKind::Blob => 3, // string
                    TokenKind::Comma
                    | TokenKind::Semicolon
                    | TokenKind::LParen
                    | TokenKind::RParen
                    | TokenKind::LBracket
                    | TokenKind::RBracket
                    | TokenKind::Dot
                    | TokenKind::Star
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Slash
                    | TokenKind::Percent
                    | TokenKind::Lt
                    | TokenKind::Gt
                    | TokenKind::Le
                    | TokenKind::Ge
                    | TokenKind::Eq
                    | TokenKind::EqEq
                    | TokenKind::Ne
                    | TokenKind::BangEq
                    | TokenKind::Ampersand
                    | TokenKind::Pipe
                    | TokenKind::Tilde
                    | TokenKind::LShift
                    | TokenKind::RShift
                    | TokenKind::Concat
                    | TokenKind::Arrow
                    | TokenKind::ArrowArrow => 5, // operator
                }
            };

            // Update context for next iteration
            type_context = update_type_context(
                type_context,
                &token.kind,
                &tokens,
                i,
                &mut paren_depth,
                &mut cast_paren_depth,
            );

            let (line, start) = offset_to_position(text, token.span.start);
            let length = (token.span.end - token.span.start) as u32;

            let delta_line = line - prev_line;
            let delta_start = if delta_line == 0 {
                start - prev_start
            } else {
                start
            };

            semantic_tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type,
                token_modifiers_bitset: 0,
            });

            prev_line = line;
            prev_start = start;
        }

    semantic_tokens
}

#[allow(dead_code)]
impl Backend {
    fn compute_diagnostics_with_schema(
        &self,
        text: &str,
        external_schema: Option<&Schema>,
    ) -> (Vec<tower_lsp::lsp_types::Diagnostic>, Vec<LintResult>) {
        self.compute_diagnostics_with_program(text, &parse_program(text), external_schema)
    }

    fn compute_diagnostics_with_program(
        &self,
        text: &str,
        program_result: &std::result::Result<Program, Vec<solite_parser::ParseError>>,
        external_schema: Option<&Schema>,
    ) -> (Vec<tower_lsp::lsp_types::Diagnostic>, Vec<LintResult>) {
        let mut lsp_diagnostics = Vec::new();
        let mut all_lint_results = Vec::new();

        match program_result {
            Ok(program) => {
                // Load lint config (discovers solite-lint.toml)
                let config = LintConfig::discover();

                // Run lint system with config and external schema
                let lint_results = lint_with_config(program, text, &config, external_schema);
                for result in &lint_results {
                    lsp_diagnostics.push(self.lint_to_lsp_diagnostic(text, &result.diagnostic));
                }
                all_lint_results = lint_results;

                // Also run semantic analysis for non-lint diagnostics (unknown tables, etc.)
                let analyzer_diagnostics = analyze_with_schema(program, external_schema);
                for diag in analyzer_diagnostics {
                    lsp_diagnostics.push(self.to_lsp_diagnostic(text, &diag));
                }
            }
            Err(parse_errors) => {
                // Convert parse errors to diagnostics
                for err in parse_errors {
                    let position = err.position();
                    let (line, character) = offset_to_position(text, position);
                    lsp_diagnostics.push(tower_lsp::lsp_types::Diagnostic {
                        range: Range {
                            start: Position { line, character },
                            end: Position {
                                line,
                                character: character + 1,
                            },
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: err.to_string(),
                        ..Default::default()
                    });
                }
            }
        }

        (lsp_diagnostics, all_lint_results)
    }

    fn to_lsp_diagnostic(
        &self,
        text: &str,
        diag: &Diagnostic,
    ) -> tower_lsp::lsp_types::Diagnostic {
        let range = span_to_range(text, &diag.span);
        let severity = match diag.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        };
        tower_lsp::lsp_types::Diagnostic {
            range,
            severity: Some(severity),
            message: diag.message.clone(),
            ..Default::default()
        }
    }

    fn lint_to_lsp_diagnostic(
        &self,
        text: &str,
        diag: &LintDiagnostic,
    ) -> tower_lsp::lsp_types::Diagnostic {
        let range = span_to_range(text, &diag.span);
        let severity = match diag.severity {
            RuleSeverity::Error => DiagnosticSeverity::ERROR,
            RuleSeverity::Warning => DiagnosticSeverity::WARNING,
            RuleSeverity::Off => DiagnosticSeverity::HINT, // Should not happen
        };
        tower_lsp::lsp_types::Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(diag.rule_id.to_string())),
            message: diag.message.clone(),
            ..Default::default()
        }
    }
}

fn offset_to_position(text: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;

    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line, col)
}

fn span_to_range(text: &str, span: &Span) -> Range {
    let (start_line, start_char) = offset_to_position(text, span.start);
    let (end_line, end_char) = offset_to_position(text, span.end);
    Range {
        start: Position {
            line: start_line,
            character: start_char,
        },
        end: Position {
            line: end_line,
            character: end_char,
        },
    }
}

/// Map an offset from the joined SQL text back to the original source.
///
/// The SQL regions represent non-overlapping ranges of the original source.
/// When parsing, regions are joined with `\n`, so we need to account for
/// the accumulated offset from previous regions plus the join separators.
fn map_offset_to_source(offset: usize, regions: &[SqlRegion]) -> usize {
    if regions.is_empty() || regions.len() == 1 {
        // No mapping needed for single region (just add region start offset)
        return if let Some(r) = regions.first() {
            r.start + offset
        } else {
            offset
        };
    }

    // Track cumulative offset in joined text
    let mut joined_offset = 0usize;

    for (i, region) in regions.iter().enumerate() {
        let region_len = region.end - region.start;

        // Check if offset falls within this region
        if offset < joined_offset + region_len {
            // Offset is within this region
            let offset_within_region = offset - joined_offset;
            return region.start + offset_within_region;
        }

        joined_offset += region_len;

        // Account for the `\n` separator between regions (except after last)
        if i < regions.len() - 1 {
            if offset == joined_offset {
                // Offset is exactly at the newline separator - map to end of this region
                return region.end;
            }
            joined_offset += 1; // for the `\n` join separator
        }
    }

    // Offset is past all regions - return end of last region
    regions.last().map(|r| r.end).unwrap_or(offset)
}

/// Map a span from the joined SQL text back to the original source.
fn map_span_to_source(span: &Span, regions: &[SqlRegion]) -> Span {
    Span {
        start: map_offset_to_source(span.start, regions),
        end: map_offset_to_source(span.end, regions),
    }
}

fn ranges_overlap(a: &Range, b: &Range) -> bool {
    // Two ranges overlap if neither is entirely before the other
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
}

fn position_to_offset(text: &str, position: Position) -> usize {
    let mut offset = 0;
    let mut line = 0u32;

    for ch in text.chars() {
        if line == position.line {
            break;
        }
        offset += ch.len_utf8();
        if ch == '\n' {
            line += 1;
        }
    }

    // Add character offset within the line
    let line_start = offset;
    for (i, ch) in text[line_start..].char_indices() {
        if i as u32 >= position.character {
            break;
        }
        offset += ch.len_utf8();
    }

    offset
}

/// Check if we're in a comment that looks like a suppression directive and suggest completions.
/// Returns Some(items) if we should show suppression completions, None otherwise.
fn suggest_suppression_completions(text: &str, offset: usize) -> Option<Vec<CompletionItem>> {
    use solite_analyzer::rules;

    // Find the start of the current line
    let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_text = &text[line_start..offset];

    // Check if line starts with "--" and we're typing something that looks like "solite-ignore"
    let trimmed = line_text.trim_start();
    if !trimmed.starts_with("--") {
        return None;
    }

    // Get the text after "--"
    let after_dashes = trimmed.strip_prefix("--")?.trim_start();

    // Check if the user is typing something that could be "solite-ignore"
    // Trigger on: empty, "s", "so", "sol", ..., "solite-ignore", "solite-ignore:"
    let prefix = "solite-ignore:";
    if !prefix.starts_with(after_dashes) && !after_dashes.starts_with(prefix) {
        return None;
    }

    // If they've typed the full prefix (or more), suggest rule IDs
    if after_dashes.starts_with(prefix) {
        let after_prefix = after_dashes.strip_prefix(prefix)?.trim_start();

        // If there's already content after the colon, check if we should still complete
        // (e.g., they might be adding another rule after a comma)
        let last_part = after_prefix.rsplit(',').next().unwrap_or("").trim();

        // Suggest all rules that start with what they've typed
        let items: Vec<CompletionItem> = rules::get_all_rules()
            .iter()
            .filter(|rule| rule.id().starts_with(last_part))
            .map(|rule| CompletionItem {
                label: rule.id().to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(rule.name().to_string()),
                documentation: Some(Documentation::String(rule.description().to_string())),
                ..Default::default()
            })
            .collect();

        if items.is_empty() {
            None
        } else {
            Some(items)
        }
    } else {
        // They're still typing "solite-ignore", suggest the full directive with each rule
        let items: Vec<CompletionItem> = rules::get_all_rules()
            .iter()
            .map(|rule| {
                // Calculate what text to insert - replace from after "--" to cursor
                let insert = format!(" solite-ignore: {}", rule.id());
                CompletionItem {
                    label: format!("solite-ignore: {}", rule.id()),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some(format!("Suppress: {}", rule.name())),
                    documentation: Some(Documentation::String(rule.description().to_string())),
                    insert_text: Some(insert),
                    ..Default::default()
                }
            })
            .collect();

        Some(items)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES.to_vec(),
                                token_modifiers: TOKEN_MODIFIERS.to_vec(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        " ".to_string(),
                        ",".to_string(),
                        ".".to_string(),
                        "\n".to_string(),
                    ]),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Solite SQL LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.on_change(
            params.text_document.uri,
            params.text_document.text,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().next() {
            self.on_change(params.text_document.uri, change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        // Remove from documents
        self.documents
            .write()
            .expect("documents lock poisoned")
            .remove(&uri);

        // If this is a notebook cell, remove from notebook tracking and rebuild schema
        if let Some(notebook_path) = get_notebook_path(&uri) {
            let should_rebuild = {
                let mut notebook_cells = self
                    .notebook_cells
                    .write()
                    .expect("notebook_cells lock poisoned");
                if let Some(cells) = notebook_cells.get_mut(&notebook_path) {
                    cells.remove(&uri);
                    !cells.is_empty()
                } else {
                    false
                }
            };

            if should_rebuild {
                // Rebuild combined schema and open schema without this cell
                let notebook_cells = self
                    .notebook_cells
                    .read()
                    .expect("notebook_cells lock poisoned");
                if let Some(cells) = notebook_cells.get(&notebook_path) {
                    // Rebuild DDL schema
                    let sources: Vec<&str> = cells.values().map(|s| s.as_str()).collect();
                    let combined_schema = build_combined_schema(&sources);
                    self.notebook_schemas
                        .write()
                        .expect("notebook_schemas lock poisoned")
                        .insert(notebook_path.clone(), combined_schema);

                    // Rebuild open schema from .open commands
                    let base_path = PathBuf::from(&notebook_path)
                        .parent()
                        .map(|p| p.to_path_buf());
                    let mut external_schema = Schema::new();
                    for cell_content in cells.values() {
                        let doc = Document::parse(cell_content, true);
                        for cmd in &doc.dot_commands {
                            match cmd {
                                DotCommand::Open { path, .. } => {
                                    let db_path = if let Some(ref base) = base_path {
                                        let path_buf = PathBuf::from(path);
                                        if path_buf.is_absolute() {
                                            path_buf
                                        } else {
                                            base.join(path)
                                        }
                                    } else {
                                        PathBuf::from(path)
                                    };
                                    let provider = FileSchemaProvider::new(&db_path);
                                    if let Ok(schema) = provider.load() {
                                        external_schema.merge(schema);
                                    }
                                }
                            }
                        }
                    }
                    self.notebook_open_schemas
                        .write()
                        .expect("notebook_open_schemas lock poisoned")
                        .insert(notebook_path, external_schema);
                }
            } else {
                // No more cells, remove notebook schemas
                self.notebook_schemas
                    .write()
                    .expect("notebook_schemas lock poisoned")
                    .remove(&notebook_path);
                self.notebook_open_schemas
                    .write()
                    .expect("notebook_open_schemas lock poisoned")
                    .remove(&notebook_path);
                self.notebook_cells
                    .write()
                    .expect("notebook_cells lock poisoned")
                    .remove(&notebook_path);
            }
        } else {
            // Regular file - remove schema and open_schema
            self.schemas
                .write()
                .expect("schemas lock poisoned")
                .remove(&uri);
            self.open_schemas
                .write()
                .expect("open_schemas lock poisoned")
                .remove(&uri);
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let documents = self.documents.read().expect("documents lock poisoned");
        let Some(text) = documents.get(&uri) else {
            return Ok(None);
        };

        let tokens = self.compute_semantic_tokens(text);

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;

        // Get the document text
        let documents = self.documents.read().expect("documents lock poisoned");
        let Some(text) = documents.get(&uri) else {
            return Ok(None);
        };
        let text = text.clone();
        drop(documents);

        // Get stored lint results for this document
        let lint_results = self.lint_results.read().expect("lint_results lock poisoned");
        let Some(results) = lint_results.get(&uri) else {
            return Ok(None);
        };

        let mut actions = Vec::new();

        // Find lint results that have fixes and overlap with the requested range
        for result in results {
            if let Some(fix) = &result.fix {
                let fix_range = span_to_range(&text, &fix.span);

                // Check if this fix's range overlaps with the requested range
                if ranges_overlap(&fix_range, &params.range) {
                    // Create a workspace edit for this fix
                    let mut changes = HashMap::new();
                    changes.insert(
                        uri.clone(),
                        vec![TextEdit {
                            range: fix_range,
                            new_text: fix.replacement.clone(),
                        }],
                    );

                    let edit = WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    };

                    let action = CodeAction {
                        title: format!("Fix: {}", result.diagnostic.message),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![tower_lsp::lsp_types::Diagnostic {
                            range: span_to_range(&text, &result.diagnostic.span),
                            severity: Some(match result.diagnostic.severity {
                                RuleSeverity::Error => DiagnosticSeverity::ERROR,
                                RuleSeverity::Warning => DiagnosticSeverity::WARNING,
                                RuleSeverity::Off => DiagnosticSeverity::HINT,
                            }),
                            code: Some(NumberOrString::String(result.diagnostic.rule_id.to_string())),
                            message: result.diagnostic.message.clone(),
                            ..Default::default()
                        }]),
                        edit: Some(edit),
                        is_preferred: Some(true),
                        ..Default::default()
                    };

                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let documents = self.documents.read().expect("documents lock poisoned");
        let Some(text) = documents.get(&uri) else {
            return Ok(None);
        };

        let offset = position_to_offset(text, position);

        // Check if we're in a comment that looks like a suppression directive
        // e.g., "-- s" or "-- solite-"
        if let Some(items) = suggest_suppression_completions(text, offset) {
            return Ok(Some(CompletionResponse::Array(items)));
        }

        let ctx = detect_context(text, offset);

        // Clone text for later use (needed for INSERT column filtering)
        let text_clone = text.clone();

        // Release documents lock before acquiring schemas lock
        drop(documents);

        // Get the appropriate schema - notebook schema for cells, or combined schema for regular files
        let notebook_path = get_notebook_path(&uri);

        // Build the combined schema for completion
        let combined_schema: Option<Schema> = if let Some(ref nb_path) = notebook_path {
            // For notebook cells, combine DDL schema with .open schema
            let ddl_schema = self.notebook_schemas
                .read()
                .expect("notebook_schemas lock poisoned")
                .get(nb_path)
                .cloned();
            let open_schema = self.notebook_open_schemas
                .read()
                .expect("notebook_open_schemas lock poisoned")
                .get(nb_path)
                .cloned();

            match (ddl_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        } else {
            // For regular files, combine document schema with open_schema
            let schemas = self.schemas.read().expect("schemas lock poisoned");
            let open_schemas = self.open_schemas.read().expect("open_schemas lock poisoned");

            let doc_schema = schemas.get(&uri).cloned();
            let open_schema = open_schemas.get(&uri).cloned();

            match (doc_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    // Merge: open_schema provides external tables, doc_schema provides local tables
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        };

        let schema: Option<&Schema> = combined_schema.as_ref();

        // Use consolidated completion logic from completions.rs
        let options = ExtendedCompletionOptions {
            document_text: Some(&text_clone),
            cursor_offset: Some(offset),
            include_documentation: true,
        };
        let items = get_completions_extended(&ctx, schema, &options);

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;

        let documents = self.documents.read().expect("documents lock poisoned");
        let Some(text) = documents.get(&uri) else {
            return Ok(None);
        };

        // Build format config from LSP options
        let config = FormatConfig {
            indent_size: params.options.tab_size as usize,
            indent_style: if params.options.insert_spaces {
                IndentStyle::Spaces
            } else {
                IndentStyle::Tabs
            },
            ..Default::default()
        };

        // Format the document (handles dot commands like .open)
        let formatted = match format_document(text, &config) {
            Ok(formatted) => formatted,
            Err(_) => return Ok(None), // Return None on parse error
        };

        // If no changes, return empty edits
        if &formatted == text {
            return Ok(Some(vec![]));
        }

        // Calculate the range covering the entire document
        let lines: Vec<&str> = text.lines().collect();
        let last_line = lines.len().saturating_sub(1);
        let last_char = lines.last().map(|l| l.len()).unwrap_or(0);

        let edit = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: last_line as u32,
                    character: last_char as u32,
                },
            },
            new_text: formatted,
        };

        Ok(Some(vec![edit]))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        // For simplicity, format the whole document when range formatting is requested.
        // A more sophisticated implementation could extract and format only the selected range.
        let formatting_params = DocumentFormattingParams {
            text_document: params.text_document,
            options: params.options,
            work_done_progress_params: params.work_done_progress_params,
        };
        self.formatting(formatting_params).await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        self.client
            .log_message(MessageType::INFO, format!("Hover request at {}:{}", position.line, position.character))
            .await;

        // Get document text (scope the lock so it's dropped before any await)
        let text = {
            let documents = self.documents.read().expect("documents lock poisoned");
            documents.get(&uri).cloned()
        };

        let Some(text) = text else {
            self.client
                .log_message(MessageType::WARNING, format!("Hover: document not found: {}", uri))
                .await;
            return Ok(None);
        };

        // Get combined schema for this document (doc schema + open schema)
        let notebook_path = get_notebook_path(&uri);
        let schema: Option<Schema> = if let Some(ref nb_path) = notebook_path {
            // For notebook cells, combine DDL schema with .open schema
            let ddl_schema = self.notebook_schemas
                .read()
                .expect("notebook_schemas lock poisoned")
                .get(nb_path)
                .cloned();
            let open_schema = self.notebook_open_schemas
                .read()
                .expect("notebook_open_schemas lock poisoned")
                .get(nb_path)
                .cloned();

            match (ddl_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        } else {
            // Combine document schema with open_schema
            let schemas = self.schemas.read().expect("schemas lock poisoned");
            let open_schemas = self.open_schemas.read().expect("open_schemas lock poisoned");
            let doc_schema = schemas.get(&uri).cloned();
            let open_schema = open_schemas.get(&uri).cloned();
            match (doc_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        };

        // Parse the document (use Document::parse to handle dot commands like .open)
        let doc = Document::parse(&text, true);
        let program = match doc.program {
            Ok(p) => p,
            Err(e) => {
                self.client
                    .log_message(MessageType::WARNING, format!("Hover: parse failed: {:?}", e))
                    .await;
                return Ok(None);
            }
        };

        // Build the SQL source that was actually parsed (joined SQL regions)
        let sql_source: String = doc.sql_regions
            .iter()
            .map(|r| &text[r.start..r.end])
            .collect::<Vec<_>>()
            .join("\n");

        // Convert position to offset in ORIGINAL text
        let original_offset = position_to_offset(&text, position);

        // Map original offset to SQL source offset
        // Find which SQL region contains this offset, then calculate position within joined string
        let offset = {
            let mut sql_offset = 0;
            let mut found_offset = None;
            for (i, region) in doc.sql_regions.iter().enumerate() {
                if original_offset >= region.start && original_offset < region.end {
                    // Cursor is in this region
                    let offset_within_region = original_offset - region.start;
                    found_offset = Some(sql_offset + offset_within_region);
                    break;
                }
                // Add this region's length plus newline separator
                sql_offset += region.end - region.start;
                if i < doc.sql_regions.len() - 1 {
                    sql_offset += 1; // for the \n between regions
                }
            }
            match found_offset {
                Some(o) => o,
                None => {
                    // Cursor is not in a SQL region (maybe in a dot command line)
                    return Ok(None);
                }
            }
        };

        // Find the statement containing the cursor
        let Some(stmt) = find_statement_at_offset(&program, offset) else {
            self.client
                .log_message(MessageType::INFO, format!("Hover: no statement at offset {}", offset))
                .await;
            return Ok(None);
        };

        // Find the symbol at the cursor position (use sql_source since AST spans are relative to it)
        let Some((symbol, symbol_span)) = find_symbol_at_offset(stmt, &sql_source, offset, schema.as_ref()) else {
            self.client
                .log_message(MessageType::INFO, format!("Hover: no symbol at offset {}", offset))
                .await;
            return Ok(None);
        };

        self.client
            .log_message(MessageType::INFO, format!("Hover: found symbol {:?}", symbol))
            .await;

        // Format hover content
        let content = format_hover_content(&symbol, schema.as_ref());

        self.client
            .log_message(MessageType::INFO, format!("Hover: returning content ({} chars)", content.len()))
            .await;

        // Map symbol_span from sql_source back to original text for the range
        let original_span = map_span_to_source(&symbol_span, &doc.sql_regions);

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: Some(span_to_range(&text, &original_span)),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // Get document text
        let documents = self.documents.read().expect("documents lock poisoned");
        let Some(text) = documents.get(&uri) else {
            return Ok(None);
        };
        let text = text.clone();
        drop(documents);

        // Get combined schema for this document (doc schema + open schema)
        let notebook_path = get_notebook_path(&uri);
        let schema: Option<Schema> = if let Some(ref nb_path) = notebook_path {
            // For notebook cells, combine DDL schema with .open schema
            let ddl_schema = self.notebook_schemas
                .read()
                .expect("notebook_schemas lock poisoned")
                .get(nb_path)
                .cloned();
            let open_schema = self.notebook_open_schemas
                .read()
                .expect("notebook_open_schemas lock poisoned")
                .get(nb_path)
                .cloned();

            match (ddl_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        } else {
            // Combine document schema with open_schema
            let schemas = self.schemas.read().expect("schemas lock poisoned");
            let open_schemas = self.open_schemas.read().expect("open_schemas lock poisoned");
            let doc_schema = schemas.get(&uri).cloned();
            let open_schema = open_schemas.get(&uri).cloned();
            match (doc_schema, open_schema) {
                (Some(mut ds), Some(os)) => {
                    ds.merge(os);
                    Some(ds)
                }
                (Some(ds), None) => Some(ds),
                (None, Some(os)) => Some(os),
                (None, None) => None,
            }
        };

        // Parse the document
        let Ok(program) = parse_program(&text) else {
            return Ok(None);
        };

        // Convert position to offset
        let offset = position_to_offset(&text, position);

        // Find the statement containing the cursor
        let Some(stmt) = find_statement_at_offset(&program, offset) else {
            return Ok(None);
        };

        // Find the symbol at the cursor position
        let Some((symbol, _)) = find_symbol_at_offset(stmt, &text, offset, schema.as_ref()) else {
            return Ok(None);
        };

        // Get the definition span
        let Some(def_span) = get_definition_span(&symbol) else {
            return Ok(None);
        };

        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: uri.clone(),
            range: span_to_range(&text, &def_span),
        })))
    }
}

/// Run the LSP server on stdin/stdout.
///
/// This function blocks until the server is shut down.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
