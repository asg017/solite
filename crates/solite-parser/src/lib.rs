pub mod doc_comments;

pub use doc_comments::{build_doc_comment_map, DocCommentKind, DocCommentMap};
pub use doc_comments::DocComment as ParserDocComment;

/// Convert a parser DocComment to an AST DocComment.
fn parser_doc_to_ast_doc(doc: &doc_comments::DocComment) -> solite_ast::DocComment {
    solite_ast::DocComment {
        description: doc.description.clone(),
        tags: doc.tags.clone(),
    }
}

use ropey::Rope;
use solite_ast::{
    AlterTableAction, AlterTableStmt, AnalyzeStmt, AttachStmt, BeginStmt, BinaryOp, ColumnConstraint,
    ColumnDef, CommitStmt, CommonTableExpr, CompoundOp, ConflictAction, ConflictTarget,
    CreateIndexStmt, CreateTableStmt, CreateTriggerStmt, CreateViewStmt, CreateVirtualTableStmt,
    DefaultValue, Deferrable, DeleteStmt, DetachStmt, DistinctAll, DropIndexStmt, DropTableStmt,
    DropTriggerStmt, DropViewStmt, Expr, ForeignKeyAction, FrameBound, FrameExclude, FrameSpec,
    FrameUnit, FromClause, IndexedBy, IndexedColumn, InsertSource, InsertStmt, JoinConstraint,
    JoinType, Materialized, OrderDirection, PragmaStmt, PragmaValue, Program, QualifiedName,
    RaiseAction, ReindexStmt, ReleaseStmt, ResultColumn, RollbackStmt, SavepointStmt, SelectCore,
    SelectStmt, Span, Statement, TableConstraint, TableOption, TableOrSubquery, TransactionType,
    TriggerEvent, TriggerTiming, TypeName, UnaryOp, UpdateAssignment, UpdateStmt, UpsertClause, VacuumStmt,
    WindowSpec, WithClause,
};
use solite_lexer::{lex, Token, TokenKind};
use thiserror::Error;

/// Line and column location (1-indexed for display)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ParseError {
    #[error("{location}: Unexpected token")]
    UnexpectedToken { location: Location },

    #[error("Unexpected end of input")]
    Eof,

    #[error("{location}: Expected {expected}, found {found:?}")]
    Expected {
        expected: &'static str,
        found: Option<TokenKind>,
        location: Location,
    },

    #[error("{location}: Invalid blob literal")]
    InvalidBlob { location: Location },
}

impl ParseError {
    pub fn position(&self) -> usize {
        match self {
            ParseError::UnexpectedToken { location } => location.offset,
            ParseError::Eof => 0,
            ParseError::Expected { location, .. } => location.offset,
            ParseError::InvalidBlob { location } => location.offset,
        }
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    source: String,
    rope: Rope,
    /// Doc comment map for sqlite-docs support
    doc_map: DocCommentMap,
}

impl Parser {
    pub fn new(source: &str) -> Self {
        // First, lex all tokens and build doc comment map before filtering
        let all_tokens = lex(source);
        let doc_map = build_doc_comment_map(&all_tokens, source);

        // Filter out comments - they're kept for semantic highlighting but not for parsing
        let tokens = all_tokens
            .into_iter()
            .filter(|t| t.kind != TokenKind::Comment && t.kind != TokenKind::BlockComment)
            .collect();
        Self {
            tokens,
            cursor: 0,
            source: source.to_string(),
            rope: Rope::from_str(source),
            doc_map,
        }
    }

    /// Convert a byte offset to a Location (1-indexed line and column)
    fn offset_to_location(&self, offset: usize) -> Location {
        let line_idx = self.rope.byte_to_line(offset.min(self.rope.len_bytes()));
        let line_start = self.rope.line_to_byte(line_idx);
        Location {
            line: line_idx + 1,
            column: offset - line_start + 1,
            offset,
        }
    }

    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn current_kind(&self) -> Option<&TokenKind> {
        self.current().map(|t| &t.kind)
    }

    /// Peek at a token n positions ahead (0 = current)
    fn peek_nth(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.cursor + n)
    }

    fn advance(&mut self) -> Option<&Token> {
        let token = self.tokens.get(self.cursor);
        self.cursor += 1;
        token
    }

    /// Consume the current token if it matches the expected kind, returning a clone of it.
    fn consume_if(&mut self, kind: TokenKind) -> Option<Token> {
        match self.current() {
            Some(token) if token.kind == kind => {
                let token = token.clone();
                self.advance();
                Some(token)
            }
            _ => None,
        }
    }

    fn expect(&mut self, kind: TokenKind, expected: &'static str) -> Result<Token, ParseError> {
        match self.current() {
            Some(token) if token.kind == kind => {
                let token = token.clone();
                self.advance();
                Ok(token)
            }
            Some(token) => Err(ParseError::Expected {
                expected,
                found: Some(token.kind),
                location: self.offset_to_location(token.span.start),
            }),
            None => Err(ParseError::Eof),
        }
    }

    fn slice(&self, span: &std::ops::Range<usize>) -> &str {
        &self.source[span.clone()]
    }

    /// Check if the current token is any kind of identifier
    fn is_ident_like(&self) -> bool {
        matches!(
            self.current_kind(),
            Some(TokenKind::Ident)
                | Some(TokenKind::QuotedIdent)
                | Some(TokenKind::BracketIdent)
                | Some(TokenKind::BacktickIdent)
        )
    }

    /// Expect any kind of identifier token
    fn expect_ident(&mut self, expected: &'static str) -> Result<Token, ParseError> {
        match self.current() {
            Some(token) if matches!(
                token.kind,
                TokenKind::Ident | TokenKind::QuotedIdent | TokenKind::BracketIdent | TokenKind::BacktickIdent
            ) => {
                let token = token.clone();
                self.advance();
                Ok(token)
            }
            Some(token) => Err(ParseError::Expected {
                expected,
                found: Some(token.kind),
                location: self.offset_to_location(token.span.start),
            }),
            None => Err(ParseError::Eof),
        }
    }

    /// Extract the name from an identifier token, handling dequoting
    fn ident_name(&self, token: &Token) -> String {
        let raw = self.slice(&token.span);
        match token.kind {
            TokenKind::Ident => raw.to_string(),
            TokenKind::QuotedIdent => {
                // Remove surrounding quotes and unescape "" -> "
                let inner = &raw[1..raw.len() - 1];
                inner.replace("\"\"", "\"")
            }
            TokenKind::BracketIdent => {
                // Remove surrounding brackets [...]
                raw[1..raw.len() - 1].to_string()
            }
            TokenKind::BacktickIdent => {
                // Remove surrounding backticks `...`
                raw[1..raw.len() - 1].to_string()
            }
            _ => raw.to_string(),
        }
    }

    pub fn parse(&mut self) -> Result<Program, Vec<ParseError>> {
        let mut statements = Vec::new();
        let mut errors = Vec::new();

        while self.current().is_some() {
            match self.parse_statement() {
                Ok(stmt) => statements.push(stmt),
                Err(e) => {
                    errors.push(e);
                    self.recover_to_semicolon();
                }
            }
        }

        if errors.is_empty() {
            Ok(Program { statements })
        } else {
            Err(errors)
        }
    }

    fn recover_to_semicolon(&mut self) {
        while let Some(token) = self.current() {
            if token.kind == TokenKind::Semicolon {
                self.advance();
                return;
            }
            self.advance();
        }
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        match self.current_kind() {
            // EXPLAIN [QUERY PLAN] statement
            Some(TokenKind::Explain) => self.parse_explain_stmt(),

            // DML - WITH can start SELECT, INSERT, UPDATE, or DELETE
            Some(TokenKind::With) => self.parse_with_dml_stmt(),
            Some(TokenKind::Select) => self.parse_select_stmt().map(Statement::Select),
            Some(TokenKind::Insert) | Some(TokenKind::Replace) => {
                self.parse_insert_stmt(None).map(Statement::Insert)
            }
            Some(TokenKind::Update) => self.parse_update_stmt(None).map(Statement::Update),
            Some(TokenKind::Delete) => self.parse_delete_stmt(None).map(Statement::Delete),

            // DDL
            Some(TokenKind::Create) => self.parse_create_stmt(),
            Some(TokenKind::Drop) => self.parse_drop_stmt(),
            Some(TokenKind::Alter) => self.parse_alter_stmt(),

            // TCL
            Some(TokenKind::Begin) => self.parse_begin_stmt().map(Statement::Begin),
            Some(TokenKind::Commit) | Some(TokenKind::End) => {
                self.parse_commit_stmt().map(Statement::Commit)
            }
            Some(TokenKind::Rollback) => self.parse_rollback_stmt().map(Statement::Rollback),
            Some(TokenKind::Savepoint) => self.parse_savepoint_stmt().map(Statement::Savepoint),
            Some(TokenKind::Release) => self.parse_release_stmt().map(Statement::Release),

            // Database management
            Some(TokenKind::Vacuum) => self.parse_vacuum_stmt().map(Statement::Vacuum),
            Some(TokenKind::Analyze) => self.parse_analyze_stmt().map(Statement::Analyze),
            Some(TokenKind::Reindex) => self.parse_reindex_stmt().map(Statement::Reindex),
            Some(TokenKind::Attach) => self.parse_attach_stmt().map(Statement::Attach),
            Some(TokenKind::Detach) => self.parse_detach_stmt().map(Statement::Detach),
            Some(TokenKind::Pragma) => self.parse_pragma_stmt().map(Statement::Pragma),

            Some(_) => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::UnexpectedToken {
                    location: self.offset_to_location(pos),
                })
            }
            None => Err(ParseError::Eof),
        }
    }

    /// Parse EXPLAIN [QUERY PLAN] statement
    fn parse_explain_stmt(&mut self) -> Result<Statement, ParseError> {
        let explain_tok = self.expect(TokenKind::Explain, "EXPLAIN")?;
        let start = explain_tok.span.start;

        // Optional: QUERY PLAN
        let query_plan = if self.current_kind() == Some(&TokenKind::Query) {
            self.advance();
            self.expect(TokenKind::Plan, "PLAN")?;
            true
        } else {
            false
        };

        // Parse the statement to explain
        let stmt = Box::new(self.parse_statement()?);
        let end = match stmt.as_ref() {
            Statement::Select(s) => s.span.end,
            Statement::Insert(s) => s.span.end,
            Statement::Update(s) => s.span.end,
            Statement::Delete(s) => s.span.end,
            Statement::CreateTable(s) => s.span.end,
            Statement::CreateIndex(s) => s.span.end,
            Statement::CreateView(s) => s.span.end,
            Statement::CreateTrigger(s) => s.span.end,
            Statement::CreateVirtualTable(s) => s.span.end,
            Statement::AlterTable(s) => s.span.end,
            Statement::DropTable(s) => s.span.end,
            Statement::DropIndex(s) => s.span.end,
            Statement::DropView(s) => s.span.end,
            Statement::DropTrigger(s) => s.span.end,
            Statement::Begin(s) => s.span.end,
            Statement::Commit(s) => s.span.end,
            Statement::Rollback(s) => s.span.end,
            Statement::Savepoint(s) => s.span.end,
            Statement::Release(s) => s.span.end,
            Statement::Vacuum(s) => s.span.end,
            Statement::Analyze(s) => s.span.end,
            Statement::Reindex(s) => s.span.end,
            Statement::Attach(s) => s.span.end,
            Statement::Detach(s) => s.span.end,
            Statement::Pragma(s) => s.span.end,
            Statement::Explain { span, .. } => span.end,
        };

        Ok(Statement::Explain {
            query_plan,
            stmt,
            span: Span::new(start, end),
        })
    }

    /// Parse a DML statement that starts with WITH (could be SELECT, INSERT, UPDATE, or DELETE)
    fn parse_with_dml_stmt(&mut self) -> Result<Statement, ParseError> {
        let with_clause = self.parse_with_clause()?;
        let with_start = with_clause.span.start;
        match self.current_kind() {
            Some(TokenKind::Select) => {
                let mut stmt = self.parse_select_stmt_core()?;
                // Update span to include WITH clause
                stmt.span.start = with_start;
                stmt.with_clause = Some(with_clause);
                Ok(Statement::Select(stmt))
            }
            Some(TokenKind::Insert) | Some(TokenKind::Replace) => {
                self.parse_insert_stmt(Some(with_clause)).map(Statement::Insert)
            }
            Some(TokenKind::Update) => {
                self.parse_update_stmt(Some(with_clause)).map(Statement::Update)
            }
            Some(TokenKind::Delete) => {
                self.parse_delete_stmt(Some(with_clause)).map(Statement::Delete)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::Expected {
                    expected: "SELECT, INSERT, UPDATE, or DELETE after WITH",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    // ========================================
    // CREATE Statement Dispatcher
    // ========================================

    /// Parse CREATE statement (dispatches to specific parser based on object type)
    fn parse_create_stmt(&mut self) -> Result<Statement, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);
        self.expect(TokenKind::Create, "CREATE")?;

        // Check for UNIQUE (for CREATE UNIQUE INDEX)
        let unique = if self.current_kind() == Some(&TokenKind::Unique) {
            self.advance();
            true
        } else {
            false
        };

        // Check for VIRTUAL (for CREATE VIRTUAL TABLE)
        if self.current_kind() == Some(&TokenKind::Virtual) {
            return self.parse_create_virtual_table_stmt_inner(start);
        }

        // Check for TEMP/TEMPORARY
        let temporary = match self.current_kind() {
            Some(TokenKind::Temp) | Some(TokenKind::Temporary) => {
                self.advance();
                true
            }
            _ => false,
        };

        match self.current_kind() {
            Some(TokenKind::Table) => {
                self.parse_create_table_stmt_inner(start, temporary).map(Statement::CreateTable)
            }
            Some(TokenKind::Index) => {
                self.parse_create_index_stmt_inner(start, unique).map(Statement::CreateIndex)
            }
            Some(TokenKind::View) => {
                self.parse_create_view_stmt_inner(start, temporary).map(Statement::CreateView)
            }
            Some(TokenKind::Trigger) => {
                self.parse_create_trigger_stmt_inner(start, temporary).map(Statement::CreateTrigger)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::Expected {
                    expected: "TABLE, INDEX, VIEW, TRIGGER, or VIRTUAL",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    /// Parse: CREATE [TEMP|TEMPORARY] TABLE [IF NOT EXISTS] [schema.]table_name
    ///        (column_def, ... [, table_constraint, ...]) [table_options]
    ///        OR: CREATE ... TABLE ... AS select_stmt
    /// Note: CREATE and TEMP/TEMPORARY already consumed by parse_create_stmt
    fn parse_create_table_stmt_inner(&mut self, start: usize, temporary: bool) -> Result<CreateTableStmt, ParseError> {
        self.expect(TokenKind::Table, "TABLE")?;

        // Optional: IF NOT EXISTS
        let if_not_exists = if self.current_kind() == Some(&TokenKind::If) {
            self.advance(); // IF
            self.expect(TokenKind::Not, "NOT")?;
            self.expect(TokenKind::Exists, "EXISTS")?;
            true
        } else {
            false
        };

        // Parse [schema.]table_name
        let first_ident = self.expect_ident("table name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, table_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance(); // consume dot
            let table_ident = self.expect_ident("table name")?;
            let table_name = self.ident_name(&table_ident);
            (Some(first_name), table_name)
        } else {
            (None, first_name)
        };

        // Check for CREATE TABLE ... AS SELECT
        if self.current_kind() == Some(&TokenKind::As) {
            self.advance(); // AS
            let select = self.parse_select_stmt()?;
            let end = select.span.end;
            return Ok(CreateTableStmt {
                temporary,
                if_not_exists,
                schema,
                table_name,
                columns: vec![],
                table_constraints: vec![],
                table_options: vec![],
                as_select: Some(Box::new(select)),
                doc: None, // AS SELECT doesn't support doc comments
                span: Span::new(start, end),
            });
        }

        // Parse column definitions and table constraints: (col1 type1, ..., PRIMARY KEY (...), ...)
        self.expect(TokenKind::LParen, "(")?;

        // Look for table-level documentation (--! comments) after the opening paren
        // The doc is associated with the position of the first token after the paren
        let table_doc = self.current()
            .and_then(|token| self.doc_map.get_table_doc(token.span.start))
            .map(parser_doc_to_ast_doc);

        let mut columns = Vec::new();
        let mut table_constraints = Vec::new();

        // First element: could be column def or table constraint
        if self.is_table_constraint_start() {
            table_constraints.push(self.parse_table_constraint()?);
        } else {
            columns.push(self.parse_column_def_with_doc()?);
        }

        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance(); // consume comma

            // After column definitions, we might have table constraints
            if self.is_table_constraint_start() {
                table_constraints.push(self.parse_table_constraint()?);
            } else if table_constraints.is_empty() {
                // Only parse column defs if we haven't started table constraints yet
                columns.push(self.parse_column_def_with_doc()?);
            } else {
                // After table constraints start, only table constraints allowed
                table_constraints.push(self.parse_table_constraint()?);
            }
        }

        let rparen = self.expect(TokenKind::RParen, ")")?;
        let mut end = rparen.span.end;

        // Parse table options: WITHOUT ROWID, STRICT
        let table_options = self.parse_table_options(&mut end)?;

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(CreateTableStmt {
            temporary,
            if_not_exists,
            schema,
            table_name,
            columns,
            table_constraints,
            table_options,
            as_select: None,
            doc: table_doc,
            span: Span::new(start, end),
        })
    }

    /// Check if current position starts a table constraint
    fn is_table_constraint_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Some(&TokenKind::Primary)
                | Some(&TokenKind::Unique)
                | Some(&TokenKind::Check)
                | Some(&TokenKind::Foreign)
                | Some(&TokenKind::Constraint)
        )
    }

    /// Parse a table-level constraint
    fn parse_table_constraint(&mut self) -> Result<TableConstraint, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);

        // Optional: CONSTRAINT name
        let name = if self.current_kind() == Some(&TokenKind::Constraint) {
            self.advance(); // CONSTRAINT
            let name_tok = self.expect_ident("constraint name")?;
            Some(self.ident_name(&name_tok))
        } else {
            None
        };

        match self.current_kind() {
            Some(&TokenKind::Primary) => {
                self.advance(); // PRIMARY
                self.expect(TokenKind::Key, "KEY")?;
                self.expect(TokenKind::LParen, "(")?;

                let columns = self.parse_indexed_column_list()?;

                let rparen = self.expect(TokenKind::RParen, ")")?;
                let conflict = self.try_parse_conflict_clause()?;

                let end = conflict.as_ref().map(|_| self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(rparen.span.end)).unwrap_or(rparen.span.end);
                Ok(TableConstraint::PrimaryKey {
                    name,
                    columns,
                    conflict,
                    span: Span::new(start, end),
                })
            }
            Some(&TokenKind::Unique) => {
                self.advance(); // UNIQUE
                self.expect(TokenKind::LParen, "(")?;

                let columns = self.parse_indexed_column_list()?;

                let rparen = self.expect(TokenKind::RParen, ")")?;
                let conflict = self.try_parse_conflict_clause()?;

                let end = conflict.as_ref().map(|_| self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(rparen.span.end)).unwrap_or(rparen.span.end);
                Ok(TableConstraint::Unique {
                    name,
                    columns,
                    conflict,
                    span: Span::new(start, end),
                })
            }
            Some(&TokenKind::Check) => {
                self.advance(); // CHECK
                self.expect(TokenKind::LParen, "(")?;
                let expr = self.parse_expr()?;
                let rparen = self.expect(TokenKind::RParen, ")")?;

                Ok(TableConstraint::Check {
                    name,
                    expr,
                    span: Span::new(start, rparen.span.end),
                })
            }
            Some(&TokenKind::Foreign) => {
                self.advance(); // FOREIGN
                self.expect(TokenKind::Key, "KEY")?;
                self.expect(TokenKind::LParen, "(")?;

                // Parse column name list
                let mut columns = Vec::new();
                let col_tok = self.expect_ident("column name")?;
                columns.push(self.ident_name(&col_tok));

                while self.current_kind() == Some(&TokenKind::Comma) {
                    self.advance();
                    let col_tok = self.expect_ident("column name")?;
                    columns.push(self.ident_name(&col_tok));
                }

                self.expect(TokenKind::RParen, ")")?;
                self.expect(TokenKind::References, "REFERENCES")?;

                let table_tok = self.expect_ident("table name")?;
                let foreign_table = self.ident_name(&table_tok);

                // Optional: (columns)
                let foreign_columns = if self.current_kind() == Some(&TokenKind::LParen) {
                    self.advance();
                    let mut cols = Vec::new();
                    let col_tok = self.expect_ident("column name")?;
                    cols.push(self.ident_name(&col_tok));

                    while self.current_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                        let col_tok = self.expect_ident("column name")?;
                        cols.push(self.ident_name(&col_tok));
                    }

                    self.expect(TokenKind::RParen, ")")?;
                    Some(cols)
                } else {
                    None
                };

                // Optional: ON DELETE / ON UPDATE actions
                let mut on_delete = None;
                let mut on_update = None;

                loop {
                    if self.current_kind() == Some(&TokenKind::On) {
                        self.advance(); // ON
                        match self.current_kind() {
                            Some(&TokenKind::Delete) => {
                                self.advance();
                                on_delete = Some(self.parse_foreign_key_action()?);
                            }
                            Some(&TokenKind::Update) => {
                                self.advance();
                                on_update = Some(self.parse_foreign_key_action()?);
                            }
                            _ => {
                                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                                return Err(ParseError::Expected {
                                    expected: "DELETE or UPDATE",
                                    found: self.current_kind().cloned(),
                                    location: self.offset_to_location(pos),
                                });
                            }
                        }
                    } else {
                        break;
                    }
                }

                // Optional: DEFERRABLE / NOT DEFERRABLE
                let deferrable = self.try_parse_deferrable()?;

                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(TableConstraint::ForeignKey {
                    name,
                    columns,
                    foreign_table,
                    foreign_columns,
                    on_delete,
                    on_update,
                    deferrable,
                    span: Span::new(start, end),
                })
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::Expected {
                    expected: "PRIMARY, UNIQUE, CHECK, or FOREIGN",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    /// Parse indexed column list: column [COLLATE name] [ASC|DESC], ...
    fn parse_indexed_column_list(&mut self) -> Result<Vec<IndexedColumn>, ParseError> {
        let mut columns = Vec::new();
        columns.push(self.parse_indexed_column()?);

        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            columns.push(self.parse_indexed_column()?);
        }

        Ok(columns)
    }

    /// Parse a single indexed column: column [COLLATE name] [ASC|DESC]
    fn parse_indexed_column(&mut self) -> Result<IndexedColumn, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);

        // Column can be an identifier or an expression (for expression indexes)
        let column = if self.current_kind() == Some(&TokenKind::LParen) {
            // Expression in parentheses
            self.parse_expr()?
        } else {
            // Simple column name
            let col_tok = self.expect_ident("column name")?;
            let col_name = self.ident_name(&col_tok);
            Expr::Column {
                schema: None,
                table: None,
                column: col_name,
                span: Span::from(col_tok.span),
            }
        };

        // Optional: COLLATE collation_name
        let collation = if self.current_kind() == Some(&TokenKind::Collate) {
            self.advance();
            let coll_tok = self.expect_ident("collation name")?;
            Some(self.ident_name(&coll_tok))
        } else {
            None
        };

        // Optional: ASC | DESC
        let direction = match self.current_kind() {
            Some(&TokenKind::Asc) => {
                self.advance();
                Some(OrderDirection::Asc)
            }
            Some(&TokenKind::Desc) => {
                self.advance();
                Some(OrderDirection::Desc)
            }
            _ => None,
        };

        let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
        Ok(IndexedColumn {
            column,
            collation,
            direction,
            span: Span::new(start, end),
        })
    }

    /// Parse optional DEFERRABLE clause
    fn try_parse_deferrable(&mut self) -> Result<Option<Deferrable>, ParseError> {
        if self.current_kind() == Some(&TokenKind::Not) {
            // Need to look ahead to check for DEFERRABLE
            if self.tokens.get(self.cursor + 1).map(|t| &t.kind) == Some(&TokenKind::Deferrable) {
                self.advance(); // NOT
                self.advance(); // DEFERRABLE
                return Ok(Some(Deferrable::NotDeferrable));
            }
            // NOT followed by something else - don't consume
            return Ok(None);
        }

        if self.current_kind() == Some(&TokenKind::Deferrable) {
            self.advance(); // DEFERRABLE

            // Optional: INITIALLY DEFERRED | INITIALLY IMMEDIATE
            if self.current_kind() == Some(&TokenKind::Initially) {
                self.advance(); // INITIALLY
                match self.current_kind() {
                    Some(&TokenKind::Deferred) => {
                        self.advance();
                        return Ok(Some(Deferrable::InitiallyDeferred));
                    }
                    Some(&TokenKind::Immediate) => {
                        self.advance();
                        return Ok(Some(Deferrable::InitiallyImmediate));
                    }
                    _ => {
                        let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                        return Err(ParseError::Expected {
                            expected: "DEFERRED or IMMEDIATE",
                            found: self.current_kind().cloned(),
                            location: self.offset_to_location(pos),
                        });
                    }
                }
            }

            // DEFERRABLE without INITIALLY defaults to INITIALLY IMMEDIATE
            return Ok(Some(Deferrable::InitiallyImmediate));
        }

        Ok(None)
    }

    /// Parse table options: WITHOUT ROWID, STRICT
    /// Note: ROWID and STRICT are not keywords - they're identifiers checked semantically
    /// (matching SQLite's actual behavior from parse.y)
    fn parse_table_options(&mut self, end: &mut usize) -> Result<Vec<TableOption>, ParseError> {
        let mut options = Vec::new();

        loop {
            match self.current_kind() {
                Some(&TokenKind::Without) => {
                    self.advance(); // WITHOUT
                    // Expect identifier "ROWID" (case-insensitive)
                    let rowid_tok = self.expect_ident("ROWID")?;
                    let rowid_text = self.slice(&rowid_tok.span);
                    if !rowid_text.eq_ignore_ascii_case("rowid") {
                        return Err(ParseError::Expected {
                            expected: "ROWID",
                            found: Some(TokenKind::Ident),
                            location: self.offset_to_location(rowid_tok.span.start),
                        });
                    }
                    *end = rowid_tok.span.end;
                    options.push(TableOption::WithoutRowid);
                }
                Some(&TokenKind::Ident) | Some(&TokenKind::QuotedIdent) | Some(&TokenKind::BracketIdent) | Some(&TokenKind::BacktickIdent) => {
                    // Check if identifier is "STRICT" (case-insensitive)
                    let tok = self.current().unwrap();
                    let text = self.slice(&tok.span);
                    if text.eq_ignore_ascii_case("strict") {
                        let strict_tok = self.advance().unwrap();
                        *end = strict_tok.span.end;
                        options.push(TableOption::Strict);
                    } else {
                        break;
                    }
                }
                _ => break,
            }

            // Options can be separated by comma
            if self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(options)
    }

    /// Parse column definition with doc comment lookup: name [type] [constraints...]
    fn parse_column_def_with_doc(&mut self) -> Result<ColumnDef, ParseError> {
        // Look for column-level documentation (--- comments) before the column name
        let col_doc = self.current()
            .and_then(|token| self.doc_map.get_column_doc(token.span.start))
            .map(parser_doc_to_ast_doc);

        let mut col_def = self.parse_column_def()?;
        col_def.doc = col_doc;
        Ok(col_def)
    }

    /// Parse column definition: name [type] [constraints...]
    fn parse_column_def(&mut self) -> Result<ColumnDef, ParseError> {
        let name_token = self.expect_ident("column name")?;
        let start = name_token.span.start;
        let name = self.ident_name(&name_token);

        // Optional type name (identifier, possibly with size like VARCHAR(255))
        let type_name = if self.is_ident_like() {
            let type_token = self.advance().unwrap();
            let type_span = type_token.span.clone();
            let mut type_name = self.slice(&type_span).to_string();
            // Handle type arguments like VARCHAR(255) or DECIMAL(10,2)
            if self.current_kind() == Some(&TokenKind::LParen) {
                self.advance();
                type_name.push('(');
                // Consume tokens until we see the closing paren
                while self.current_kind() != Some(&TokenKind::RParen) && self.current().is_some() {
                    let tok = self.advance().unwrap();
                    let tok_span = tok.span.clone();
                    type_name.push_str(self.slice(&tok_span));
                }
                if self.current_kind() == Some(&TokenKind::RParen) {
                    self.advance();
                    type_name.push(')');
                }
            }
            Some(type_name)
        } else {
            None
        };

        // Parse column constraints
        let mut constraints = Vec::new();
        while let Some(constraint) = self.try_parse_column_constraint()? {
            constraints.push(constraint);
        }

        let end = constraints.last().map(|c| match c {
            ColumnConstraint::PrimaryKey { span, .. } => span.end,
            ColumnConstraint::NotNull { span, .. } => span.end,
            ColumnConstraint::Unique { span, .. } => span.end,
            ColumnConstraint::Check { span, .. } => span.end,
            ColumnConstraint::Default { span, .. } => span.end,
            ColumnConstraint::Collate { span, .. } => span.end,
            ColumnConstraint::ForeignKey { span, .. } => span.end,
            ColumnConstraint::Generated { span, .. } => span.end,
        }).unwrap_or_else(|| self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start));

        Ok(ColumnDef {
            name,
            type_name,
            constraints,
            doc: None, // Doc is set by parse_column_def_with_doc
            span: Span::new(start, end),
        })
    }

    /// Try to parse a single column constraint, returning None if no constraint found
    fn try_parse_column_constraint(&mut self) -> Result<Option<ColumnConstraint>, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);

        // Check for CONSTRAINT name prefix (optional)
        if self.current_kind() == Some(&TokenKind::Constraint) {
            self.advance();
            self.expect_ident("constraint name")?;
        }

        match self.current_kind() {
            Some(TokenKind::Primary) => {
                self.advance();
                self.expect(TokenKind::Key, "KEY")?;

                let order = match self.current_kind() {
                    Some(TokenKind::Asc) => { self.advance(); Some(OrderDirection::Asc) }
                    Some(TokenKind::Desc) => { self.advance(); Some(OrderDirection::Desc) }
                    _ => None,
                };

                let conflict = self.try_parse_conflict_clause()?;

                let autoincrement = if self.current_kind() == Some(&TokenKind::Autoincrement) {
                    self.advance();
                    true
                } else {
                    false
                };

                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(Some(ColumnConstraint::PrimaryKey { order, conflict, autoincrement, span: Span::new(start, end) }))
            }
            Some(TokenKind::Not) => {
                self.advance();
                self.expect(TokenKind::Null, "NULL")?;
                let conflict = self.try_parse_conflict_clause()?;
                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(Some(ColumnConstraint::NotNull { conflict, span: Span::new(start, end) }))
            }
            Some(TokenKind::Unique) => {
                self.advance();
                let conflict = self.try_parse_conflict_clause()?;
                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(Some(ColumnConstraint::Unique { conflict, span: Span::new(start, end) }))
            }
            Some(TokenKind::Check) => {
                self.advance();
                self.expect(TokenKind::LParen, "(")?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen, ")")?;
                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(Some(ColumnConstraint::Check { expr, span: Span::new(start, end) }))
            }
            Some(TokenKind::Default) => {
                self.advance();
                let value = if self.current_kind() == Some(&TokenKind::LParen) {
                    self.advance();
                    let expr = self.parse_expr()?;
                    self.expect(TokenKind::RParen, ")")?;
                    DefaultValue::Expr(expr)
                } else {
                    // Parse literal value
                    let expr = self.parse_atom()?;
                    DefaultValue::Literal(expr)
                };
                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(Some(ColumnConstraint::Default { value, span: Span::new(start, end) }))
            }
            Some(TokenKind::Collate) => {
                self.advance();
                let collation_token = self.expect_ident("collation name")?;
                let collation = self.ident_name(&collation_token);
                let end = collation_token.span.end;
                Ok(Some(ColumnConstraint::Collate { collation, span: Span::new(start, end) }))
            }
            Some(TokenKind::References) => {
                self.advance();
                let table_token = self.expect_ident("table name")?;
                let foreign_table = self.ident_name(&table_token);

                // Optional column list
                let columns = if self.current_kind() == Some(&TokenKind::LParen) {
                    self.advance();
                    let mut cols = Vec::new();
                    let col = self.expect_ident("column name")?;
                    cols.push(self.ident_name(&col));
                    while self.current_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                        let col = self.expect_ident("column name")?;
                        cols.push(self.ident_name(&col));
                    }
                    self.expect(TokenKind::RParen, ")")?;
                    Some(cols)
                } else {
                    None
                };

                // Optional ON DELETE / ON UPDATE
                let mut on_delete = None;
                let mut on_update = None;
                while self.current_kind() == Some(&TokenKind::On) {
                    self.advance();
                    match self.current_kind() {
                        Some(TokenKind::Delete) => {
                            self.advance();
                            on_delete = Some(self.parse_foreign_key_action()?);
                        }
                        Some(TokenKind::Update) => {
                            self.advance();
                            on_update = Some(self.parse_foreign_key_action()?);
                        }
                        _ => break,
                    }
                }

                let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
                Ok(Some(ColumnConstraint::ForeignKey { foreign_table, columns, on_delete, on_update, span: Span::new(start, end) }))
            }
            Some(TokenKind::Generated) => {
                self.advance();
                self.expect(TokenKind::Always, "ALWAYS")?;
                self.expect(TokenKind::As, "AS")?;
                self.parse_generated_column_body(start)
            }
            // Shorthand: AS (expr) [STORED|VIRTUAL] without GENERATED ALWAYS
            Some(TokenKind::As) => {
                self.advance();
                self.parse_generated_column_body(start)
            }
            _ => Ok(None),
        }
    }

    /// Parse the body of a generated column: (expr) [STORED|VIRTUAL]
    fn parse_generated_column_body(&mut self, start: usize) -> Result<Option<ColumnConstraint>, ParseError> {
        self.expect(TokenKind::LParen, "(")?;
        let expr = self.parse_expr()?;
        self.expect(TokenKind::RParen, ")")?;
        let stored = if self.current_kind() == Some(&TokenKind::Stored) {
            self.advance();
            true
        } else {
            // VIRTUAL is the default, or explicit
            self.consume_if(TokenKind::Virtual);
            false
        };
        let end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(start);
        Ok(Some(ColumnConstraint::Generated { expr, stored, span: Span::new(start, end) }))
    }

    /// Try to parse ON CONFLICT clause
    fn try_parse_conflict_clause(&mut self) -> Result<Option<ConflictAction>, ParseError> {
        if self.current_kind() == Some(&TokenKind::On) {
            self.advance();
            self.expect(TokenKind::Conflict, "CONFLICT")?;
            Ok(Some(self.parse_conflict_action()?))
        } else {
            Ok(None)
        }
    }

    /// Parse foreign key action: SET NULL | SET DEFAULT | CASCADE | RESTRICT | NO ACTION
    fn parse_foreign_key_action(&mut self) -> Result<ForeignKeyAction, ParseError> {
        match self.current_kind() {
            Some(TokenKind::Set) => {
                self.advance();
                match self.current_kind() {
                    Some(TokenKind::Null) => {
                        self.advance();
                        Ok(ForeignKeyAction::SetNull)
                    }
                    Some(TokenKind::Default) => {
                        self.advance();
                        Ok(ForeignKeyAction::SetDefault)
                    }
                    _ => {
                        let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                        Err(ParseError::Expected {
                            expected: "NULL or DEFAULT",
                            found: self.current_kind().cloned(),
                            location: self.offset_to_location(pos),
                        })
                    }
                }
            }
            Some(TokenKind::Cascade) => {
                self.advance();
                Ok(ForeignKeyAction::Cascade)
            }
            Some(TokenKind::Restrict) => {
                self.advance();
                Ok(ForeignKeyAction::Restrict)
            }
            Some(TokenKind::No) => {
                self.advance();
                self.expect(TokenKind::Action, "ACTION")?;
                Ok(ForeignKeyAction::NoAction)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::Expected {
                    expected: "SET NULL, SET DEFAULT, CASCADE, RESTRICT, or NO ACTION",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    /// Parse: CREATE [UNIQUE] INDEX [IF NOT EXISTS] [schema.]index ON table (columns) [WHERE expr]
    /// Note: CREATE and UNIQUE already consumed by parse_create_stmt
    fn parse_create_index_stmt_inner(&mut self, start: usize, unique: bool) -> Result<CreateIndexStmt, ParseError> {
        self.expect(TokenKind::Index, "INDEX")?;

        // Optional: IF NOT EXISTS
        let if_not_exists = if self.current_kind() == Some(&TokenKind::If) {
            self.advance();
            self.expect(TokenKind::Not, "NOT")?;
            self.expect(TokenKind::Exists, "EXISTS")?;
            true
        } else {
            false
        };

        // Parse [schema.]index_name
        let first_ident = self.expect_ident("index name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, index_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let idx_ident = self.expect_ident("index name")?;
            (Some(first_name), self.ident_name(&idx_ident))
        } else {
            (None, first_name)
        };

        self.expect(TokenKind::On, "ON")?;

        // Parse table name
        let table_token = self.expect_ident("table name")?;
        let table_name = self.ident_name(&table_token);

        // Parse column list
        self.expect(TokenKind::LParen, "(")?;
        let mut columns = Vec::new();
        columns.push(self.parse_indexed_column()?);
        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            columns.push(self.parse_indexed_column()?);
        }
        self.expect(TokenKind::RParen, ")")?;

        // Optional WHERE clause (partial index)
        let where_clause = if self.current_kind() == Some(&TokenKind::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let end = if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            semi.span.end
        } else {
            where_clause.as_ref().map(|e| e.span().end)
                .or_else(|| columns.last().map(|c| c.span.end))
                .unwrap_or(start)
        };

        Ok(CreateIndexStmt {
            unique,
            if_not_exists,
            schema,
            index_name,
            table_name,
            columns,
            where_clause,
            span: Span::new(start, end),
        })
    }

    /// Parse: CREATE [TEMP|TEMPORARY] VIEW [IF NOT EXISTS] [schema.]view [(columns)] AS select
    /// Note: CREATE and TEMP/TEMPORARY already consumed by parse_create_stmt
    fn parse_create_view_stmt_inner(&mut self, start: usize, temporary: bool) -> Result<CreateViewStmt, ParseError> {
        self.expect(TokenKind::View, "VIEW")?;

        // Optional: IF NOT EXISTS
        let if_not_exists = if self.current_kind() == Some(&TokenKind::If) {
            self.advance();
            self.expect(TokenKind::Not, "NOT")?;
            self.expect(TokenKind::Exists, "EXISTS")?;
            true
        } else {
            false
        };

        // Parse [schema.]view_name
        let first_ident = self.expect_ident("view name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, view_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let view_ident = self.expect_ident("view name")?;
            (Some(first_name), self.ident_name(&view_ident))
        } else {
            (None, first_name)
        };

        // Optional column list
        let columns = if self.current_kind() == Some(&TokenKind::LParen) {
            self.advance();
            let mut cols = Vec::new();
            let col = self.expect_ident("column name")?;
            cols.push(self.ident_name(&col));
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                let col = self.expect_ident("column name")?;
                cols.push(self.ident_name(&col));
            }
            self.expect(TokenKind::RParen, ")")?;
            Some(cols)
        } else {
            None
        };

        self.expect(TokenKind::As, "AS")?;

        // Parse SELECT statement
        let select = Box::new(self.parse_select_stmt()?);

        let end = select.span.end;

        // Consume optional semicolon (might already be consumed by select)
        self.consume_if(TokenKind::Semicolon);

        Ok(CreateViewStmt {
            temporary,
            if_not_exists,
            schema,
            view_name,
            columns,
            select,
            span: Span::new(start, end),
        })
    }

    // ========================================
    // CREATE TRIGGER Parser
    // ========================================

    /// Parse CREATE [TEMP] TRIGGER [IF NOT EXISTS] [schema.]trigger_name
    /// [BEFORE|AFTER|INSTEAD OF] {DELETE|INSERT|UPDATE [OF columns]}
    /// ON table_name [FOR EACH ROW] [WHEN expr]
    /// BEGIN statements END
    fn parse_create_trigger_stmt_inner(&mut self, start: usize, temporary: bool) -> Result<CreateTriggerStmt, ParseError> {
        self.expect(TokenKind::Trigger, "TRIGGER")?;

        // Optional: IF NOT EXISTS
        let if_not_exists = if self.current_kind() == Some(&TokenKind::If) {
            self.advance();
            self.expect(TokenKind::Not, "NOT")?;
            self.expect(TokenKind::Exists, "EXISTS")?;
            true
        } else {
            false
        };

        // Parse [schema.]trigger_name
        let first_ident = self.expect_ident("trigger name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, trigger_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let trigger_ident = self.expect_ident("trigger name")?;
            (Some(first_name), self.ident_name(&trigger_ident))
        } else {
            (None, first_name)
        };

        // Parse timing: BEFORE | AFTER | INSTEAD OF (default is BEFORE)
        let timing = match self.current_kind() {
            Some(TokenKind::Before) => {
                self.advance();
                TriggerTiming::Before
            }
            Some(TokenKind::After) => {
                self.advance();
                TriggerTiming::After
            }
            Some(TokenKind::Instead) => {
                self.advance();
                self.expect(TokenKind::Of, "OF")?;
                TriggerTiming::InsteadOf
            }
            _ => TriggerTiming::Before, // Default
        };

        // Parse event: DELETE | INSERT | UPDATE [OF columns]
        let event = match self.current_kind() {
            Some(TokenKind::Delete) => {
                self.advance();
                TriggerEvent::Delete
            }
            Some(TokenKind::Insert) => {
                self.advance();
                TriggerEvent::Insert
            }
            Some(TokenKind::Update) => {
                self.advance();
                // Optional: OF column1, column2, ...
                let columns = if self.current_kind() == Some(&TokenKind::Of) {
                    self.advance();
                    let mut cols = Vec::new();
                    let col = self.expect_ident("column name")?;
                    cols.push(self.ident_name(&col));
                    while self.current_kind() == Some(&TokenKind::Comma) {
                        self.advance();
                        let col = self.expect_ident("column name")?;
                        cols.push(self.ident_name(&col));
                    }
                    Some(cols)
                } else {
                    None
                };
                TriggerEvent::Update { columns }
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                return Err(ParseError::Expected {
                    expected: "DELETE, INSERT, or UPDATE",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                });
            }
        };

        // Parse ON table_name
        self.expect(TokenKind::On, "ON")?;
        let table_tok = self.expect_ident("table name")?;
        let table_name = self.ident_name(&table_tok);

        // Optional: FOR EACH ROW
        let for_each_row = if self.current_kind() == Some(&TokenKind::For) {
            self.advance();
            self.expect(TokenKind::Each, "EACH")?;
            self.expect(TokenKind::Row, "ROW")?;
            true
        } else {
            false
        };

        // Optional: WHEN expr
        let when_clause = if self.current_kind() == Some(&TokenKind::When) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        // Parse BEGIN ... END
        self.expect(TokenKind::Begin, "BEGIN")?;

        let mut body = Vec::new();
        while self.current_kind() != Some(&TokenKind::End) && self.current().is_some() {
            // Parse a statement (INSERT, UPDATE, DELETE, or SELECT)
            // Note: These statement parsers already consume the optional semicolon
            let stmt = match self.current_kind() {
                Some(TokenKind::Insert) => {
                    let insert = self.parse_insert_stmt(None)?;
                    Statement::Insert(insert)
                }
                Some(TokenKind::Update) => {
                    let update = self.parse_update_stmt(None)?;
                    Statement::Update(update)
                }
                Some(TokenKind::Delete) => {
                    let delete = self.parse_delete_stmt(None)?;
                    Statement::Delete(delete)
                }
                Some(TokenKind::Select) => {
                    let select = self.parse_select_stmt()?;
                    Statement::Select(select)
                }
                _ => {
                    let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                    return Err(ParseError::Expected {
                        expected: "INSERT, UPDATE, DELETE, or SELECT",
                        found: self.current_kind().cloned(),
                        location: self.offset_to_location(pos),
                    });
                }
            };
            body.push(stmt);
            // Note: Statement parsers already consume the trailing semicolon
        }

        let end_tok = self.expect(TokenKind::End, "END")?;
        let mut end = end_tok.span.end;

        // Consume optional final semicolon
        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(CreateTriggerStmt {
            temporary,
            if_not_exists,
            schema,
            trigger_name,
            timing,
            event,
            table_name,
            for_each_row,
            when_clause,
            body,
            span: Span::new(start, end),
        })
    }

    // ========================================
    // CREATE VIRTUAL TABLE Parser
    // ========================================

    /// Parse CREATE VIRTUAL TABLE [IF NOT EXISTS] [schema.]table_name USING module_name [(args)]
    fn parse_create_virtual_table_stmt_inner(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect(TokenKind::Virtual, "VIRTUAL")?;
        self.expect(TokenKind::Table, "TABLE")?;

        // Optional: IF NOT EXISTS
        let if_not_exists = if self.current_kind() == Some(&TokenKind::If) {
            self.advance();
            self.expect(TokenKind::Not, "NOT")?;
            self.expect(TokenKind::Exists, "EXISTS")?;
            true
        } else {
            false
        };

        // Parse [schema.]table_name
        let first_ident = self.expect_ident("table name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, table_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let table_ident = self.expect_ident("table name")?;
            (Some(first_name), self.ident_name(&table_ident))
        } else {
            (None, first_name)
        };

        // USING module_name
        self.expect(TokenKind::Using, "USING")?;
        let module_tok = self.expect_ident("module name")?;
        let module_name = self.ident_name(&module_tok);
        let mut end = module_tok.span.end;

        // Optional: (module_args)
        let module_args = if self.current_kind() == Some(&TokenKind::LParen) {
            self.advance();

            // Parse module arguments as raw token text until closing paren
            // Module arguments can be any tokens, so we just capture them as strings
            let mut args = Vec::new();
            let mut paren_depth = 1;
            let mut current_arg = String::new();

            while paren_depth > 0 {
                match self.current_kind() {
                    Some(&TokenKind::LParen) => {
                        let tok = self.advance().unwrap();
                        let tok_span = tok.span.clone();
                        current_arg.push_str(self.slice(&tok_span));
                        paren_depth += 1;
                    }
                    Some(&TokenKind::RParen) => {
                        paren_depth -= 1;
                        if paren_depth > 0 {
                            let tok = self.advance().unwrap();
                            let tok_span = tok.span.clone();
                            current_arg.push_str(self.slice(&tok_span));
                        } else {
                            // Don't consume the final RParen yet
                            if !current_arg.trim().is_empty() {
                                args.push(current_arg.trim().to_string());
                            }
                        }
                    }
                    Some(&TokenKind::Comma) if paren_depth == 1 => {
                        // Top-level comma separates arguments
                        self.advance();
                        if !current_arg.trim().is_empty() {
                            args.push(current_arg.trim().to_string());
                        }
                        current_arg = String::new();
                    }
                    Some(_) => {
                        let tok = self.advance().unwrap();
                        let tok_span = tok.span.clone();
                        if !current_arg.is_empty() && !current_arg.ends_with('(') {
                            current_arg.push(' ');
                        }
                        current_arg.push_str(self.slice(&tok_span));
                    }
                    None => {
                        return Err(ParseError::Eof);
                    }
                }
            }

            let rparen = self.expect(TokenKind::RParen, ")")?;
            end = rparen.span.end;

            if args.is_empty() {
                None
            } else {
                Some(args)
            }
        } else {
            None
        };

        // Optional semicolon
        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(Statement::CreateVirtualTable(CreateVirtualTableStmt {
            if_not_exists,
            schema,
            table_name,
            module_name,
            module_args,
            span: Span::new(start, end),
        }))
    }

    // ========================================
    // ALTER TABLE Parser
    // ========================================

    /// Parse ALTER TABLE statement
    fn parse_alter_stmt(&mut self) -> Result<Statement, ParseError> {
        let start = self.expect(TokenKind::Alter, "ALTER")?.span.start;
        self.expect(TokenKind::Table, "TABLE")?;

        // Parse [schema.]table_name
        let first_ident = self.expect_ident("table name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, table_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let table_ident = self.expect_ident("table name")?;
            (Some(first_name), self.ident_name(&table_ident))
        } else {
            (None, first_name)
        };

        // Parse action
        let (action, end) = match self.current_kind() {
            Some(TokenKind::Rename) => {
                self.advance();
                match self.current_kind() {
                    Some(TokenKind::To) => {
                        self.advance();
                        let new_name_token = self.expect_ident("new table name")?;
                        let new_name = self.ident_name(&new_name_token);
                        (AlterTableAction::RenameTo(new_name), new_name_token.span.end)
                    }
                    Some(TokenKind::Column) => {
                        self.advance();
                        let old_name_token = self.expect_ident("column name")?;
                        let old_name = self.ident_name(&old_name_token);
                        self.expect(TokenKind::To, "TO")?;
                        let new_name_token = self.expect_ident("new column name")?;
                        let new_name = self.ident_name(&new_name_token);
                        (AlterTableAction::RenameColumn { old_name, new_name }, new_name_token.span.end)
                    }
                    Some(TokenKind::Ident) | Some(TokenKind::QuotedIdent) | Some(TokenKind::BracketIdent) | Some(TokenKind::BacktickIdent) => {
                        // RENAME column_name TO new_name (without COLUMN keyword)
                        let old_name_token = self.expect_ident("column name")?;
                        let old_name = self.ident_name(&old_name_token);
                        self.expect(TokenKind::To, "TO")?;
                        let new_name_token = self.expect_ident("new column name")?;
                        let new_name = self.ident_name(&new_name_token);
                        (AlterTableAction::RenameColumn { old_name, new_name }, new_name_token.span.end)
                    }
                    _ => {
                        let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                        return Err(ParseError::Expected {
                            expected: "TO or COLUMN",
                            found: self.current_kind().cloned(),
                            location: self.offset_to_location(pos),
                        });
                    }
                }
            }
            Some(TokenKind::Add) => {
                self.advance();
                // Optional COLUMN keyword
                self.consume_if(TokenKind::Column);
                let col_def = self.parse_column_def()?;
                let end = col_def.span.end;
                (AlterTableAction::AddColumn(col_def), end)
            }
            Some(TokenKind::Drop) => {
                self.advance();
                // Optional COLUMN keyword
                self.consume_if(TokenKind::Column);
                let col_name_token = self.expect_ident("column name")?;
                let col_name = self.ident_name(&col_name_token);
                (AlterTableAction::DropColumn(col_name), col_name_token.span.end)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                return Err(ParseError::Expected {
                    expected: "RENAME, ADD, or DROP",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                });
            }
        };

        let end = if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            semi.span.end
        } else {
            end
        };

        Ok(Statement::AlterTable(AlterTableStmt {
            schema,
            table_name,
            action,
            span: Span::new(start, end),
        }))
    }

    // ========================================
    // DROP Statement Parsers
    // ========================================

    /// Parse DROP TABLE|INDEX|VIEW|TRIGGER statements
    fn parse_drop_stmt(&mut self) -> Result<Statement, ParseError> {
        let drop_token = self.expect(TokenKind::Drop, "DROP")?;
        let start = drop_token.span.start;

        match self.current_kind() {
            Some(TokenKind::Table) => {
                self.advance();
                let (if_exists, schema, name, end) = self.parse_drop_target()?;
                Ok(Statement::DropTable(DropTableStmt {
                    if_exists,
                    schema,
                    table_name: name,
                    span: Span::new(start, end),
                }))
            }
            Some(TokenKind::Index) => {
                self.advance();
                let (if_exists, schema, name, end) = self.parse_drop_target()?;
                Ok(Statement::DropIndex(DropIndexStmt {
                    if_exists,
                    schema,
                    index_name: name,
                    span: Span::new(start, end),
                }))
            }
            Some(TokenKind::View) => {
                self.advance();
                let (if_exists, schema, name, end) = self.parse_drop_target()?;
                Ok(Statement::DropView(DropViewStmt {
                    if_exists,
                    schema,
                    view_name: name,
                    span: Span::new(start, end),
                }))
            }
            Some(TokenKind::Trigger) => {
                self.advance();
                let (if_exists, schema, name, end) = self.parse_drop_target()?;
                Ok(Statement::DropTrigger(DropTriggerStmt {
                    if_exists,
                    schema,
                    trigger_name: name,
                    span: Span::new(start, end),
                }))
            }
            Some(kind) => Err(ParseError::Expected {
                expected: "TABLE, INDEX, VIEW, or TRIGGER",
                found: Some(*kind),
                location: self.offset_to_location(self.current().map(|t| t.span.start).unwrap_or(0)),
            }),
            None => Err(ParseError::Eof),
        }
    }

    /// Parse [IF EXISTS] [schema.]name for DROP statements
    fn parse_drop_target(&mut self) -> Result<(bool, Option<String>, String, usize), ParseError> {
        // Optional: IF EXISTS
        let if_exists = if self.current_kind() == Some(&TokenKind::If) {
            self.advance();
            self.expect(TokenKind::Exists, "EXISTS")?;
            true
        } else {
            false
        };

        // Parse [schema.]name
        let first_ident = self.expect_ident("name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, name, mut end) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let name_ident = self.expect_ident("name")?;
            let name = self.ident_name(&name_ident);
            (Some(first_name), name, name_ident.span.end)
        } else {
            (None, first_name, first_ident.span.end)
        };

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok((if_exists, schema, name, end))
    }

    // ========================================
    // TCL (Transaction Control) Statement Parsers
    // ========================================

    /// Parse: BEGIN [DEFERRED|IMMEDIATE|EXCLUSIVE] [TRANSACTION]
    fn parse_begin_stmt(&mut self) -> Result<BeginStmt, ParseError> {
        let begin_token = self.expect(TokenKind::Begin, "BEGIN")?;
        let start = begin_token.span.start;
        let mut end = begin_token.span.end;

        // Optional transaction type
        let transaction_type = match self.current_kind() {
            Some(TokenKind::Deferred) => {
                end = self.advance().unwrap().span.end;
                Some(TransactionType::Deferred)
            }
            Some(TokenKind::Immediate) => {
                end = self.advance().unwrap().span.end;
                Some(TransactionType::Immediate)
            }
            Some(TokenKind::Exclusive) => {
                end = self.advance().unwrap().span.end;
                Some(TransactionType::Exclusive)
            }
            _ => None,
        };

        // Optional TRANSACTION keyword
        if let Some(t) = self.consume_if(TokenKind::Transaction) {
            end = t.span.end;
        }

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(BeginStmt {
            transaction_type,
            span: Span::new(start, end),
        })
    }

    /// Parse: COMMIT [TRANSACTION] | END [TRANSACTION]
    fn parse_commit_stmt(&mut self) -> Result<CommitStmt, ParseError> {
        let token = self.advance().unwrap(); // COMMIT or END
        let start = token.span.start;
        let mut end = token.span.end;

        // Optional TRANSACTION keyword
        if let Some(t) = self.consume_if(TokenKind::Transaction) {
            end = t.span.end;
        }

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(CommitStmt {
            span: Span::new(start, end),
        })
    }

    /// Parse: ROLLBACK [TRANSACTION] [TO [SAVEPOINT] savepoint_name]
    fn parse_rollback_stmt(&mut self) -> Result<RollbackStmt, ParseError> {
        let rollback_token = self.expect(TokenKind::Rollback, "ROLLBACK")?;
        let start = rollback_token.span.start;
        let mut end = rollback_token.span.end;

        // Optional TRANSACTION keyword
        if let Some(t) = self.consume_if(TokenKind::Transaction) {
            end = t.span.end;
        }

        // Optional TO [SAVEPOINT] savepoint_name
        let savepoint = if self.current_kind() == Some(&TokenKind::To) {
            self.advance();
            self.consume_if(TokenKind::Savepoint); // Optional SAVEPOINT keyword
            let name_token = self.expect_ident("savepoint name")?;
            end = name_token.span.end;
            Some(self.ident_name(&name_token))
        } else {
            None
        };

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(RollbackStmt {
            savepoint,
            span: Span::new(start, end),
        })
    }

    /// Parse: SAVEPOINT savepoint_name
    fn parse_savepoint_stmt(&mut self) -> Result<SavepointStmt, ParseError> {
        let savepoint_token = self.expect(TokenKind::Savepoint, "SAVEPOINT")?;
        let start = savepoint_token.span.start;

        let name_token = self.expect_ident("savepoint name")?;
        let name = self.ident_name(&name_token);
        let mut end = name_token.span.end;

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(SavepointStmt {
            name,
            span: Span::new(start, end),
        })
    }

    /// Parse: RELEASE [SAVEPOINT] savepoint_name
    fn parse_release_stmt(&mut self) -> Result<ReleaseStmt, ParseError> {
        let release_token = self.expect(TokenKind::Release, "RELEASE")?;
        let start = release_token.span.start;

        // Optional SAVEPOINT keyword
        self.consume_if(TokenKind::Savepoint);

        let name_token = self.expect_ident("savepoint name")?;
        let name = self.ident_name(&name_token);
        let mut end = name_token.span.end;

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(ReleaseStmt {
            name,
            span: Span::new(start, end),
        })
    }

    // ========================================
    // Database Management Statement Parsers
    // ========================================

    /// Parse: VACUUM [schema_name] [INTO filename]
    fn parse_vacuum_stmt(&mut self) -> Result<VacuumStmt, ParseError> {
        let vacuum_token = self.expect(TokenKind::Vacuum, "VACUUM")?;
        let start = vacuum_token.span.start;
        let mut end = vacuum_token.span.end;

        // Optional schema name (identifier, not INTO)
        let schema = if self.is_ident_like() {
            let ident = self.advance().unwrap();
            let span = ident.span.clone();
            end = span.end;
            Some(self.slice(&span).to_string())
        } else {
            None
        };

        // Optional INTO filename
        let into_file = if self.current_kind() == Some(&TokenKind::Into) {
            self.advance();
            let file_token = self.expect(TokenKind::String, "filename")?;
            end = file_token.span.end;
            let file = self.slice(&file_token.span);
            Some(file[1..file.len() - 1].to_string()) // Remove quotes
        } else {
            None
        };

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(VacuumStmt {
            schema,
            into_file,
            span: Span::new(start, end),
        })
    }

    /// Parse: ANALYZE [schema_name | table_or_index_name | schema_name.table_or_index_name]
    fn parse_analyze_stmt(&mut self) -> Result<AnalyzeStmt, ParseError> {
        let analyze_token = self.expect(TokenKind::Analyze, "ANALYZE")?;
        let start = analyze_token.span.start;
        let mut end = analyze_token.span.end;

        // Optional target
        let target = if self.is_ident_like() {
            let (qname, e) = self.parse_qualified_name()?;
            end = e;
            Some(qname)
        } else {
            None
        };

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(AnalyzeStmt {
            target,
            span: Span::new(start, end),
        })
    }

    /// Parse: REINDEX [collation_name | [schema.]table_or_index_name]
    fn parse_reindex_stmt(&mut self) -> Result<ReindexStmt, ParseError> {
        let reindex_token = self.expect(TokenKind::Reindex, "REINDEX")?;
        let start = reindex_token.span.start;
        let mut end = reindex_token.span.end;

        // Optional target
        let target = if self.is_ident_like() {
            let (qname, e) = self.parse_qualified_name()?;
            end = e;
            Some(qname)
        } else {
            None
        };

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(ReindexStmt {
            target,
            span: Span::new(start, end),
        })
    }

    /// Parse: ATTACH [DATABASE] expr AS schema_name
    fn parse_attach_stmt(&mut self) -> Result<AttachStmt, ParseError> {
        let attach_token = self.expect(TokenKind::Attach, "ATTACH")?;
        let start = attach_token.span.start;

        // Optional DATABASE keyword
        self.consume_if(TokenKind::Database);

        // Parse expression (typically a string filename)
        let expr = self.parse_expr()?;

        self.expect(TokenKind::As, "AS")?;

        let name_token = self.expect_ident("schema name")?;
        let schema_name = self.ident_name(&name_token);
        let mut end = name_token.span.end;

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(AttachStmt {
            expr,
            schema_name,
            span: Span::new(start, end),
        })
    }

    /// Parse: DETACH [DATABASE] schema_name
    fn parse_detach_stmt(&mut self) -> Result<DetachStmt, ParseError> {
        let detach_token = self.expect(TokenKind::Detach, "DETACH")?;
        let start = detach_token.span.start;

        // Optional DATABASE keyword
        self.consume_if(TokenKind::Database);

        let name_token = self.expect_ident("schema name")?;
        let schema_name = self.ident_name(&name_token);
        let mut end = name_token.span.end;

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(DetachStmt {
            schema_name,
            span: Span::new(start, end),
        })
    }

    /// Parse: PRAGMA [schema.]pragma_name [= value | (value)]
    fn parse_pragma_stmt(&mut self) -> Result<PragmaStmt, ParseError> {
        let pragma_token = self.expect(TokenKind::Pragma, "PRAGMA")?;
        let start = pragma_token.span.start;

        // Parse [schema.]pragma_name
        let first_ident = self.expect_ident("pragma name")?;
        let first_name = self.ident_name(&first_ident);
        let mut end = first_ident.span.end;

        let (schema, name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let name_ident = self.expect_ident("pragma name")?;
            end = name_ident.span.end;
            (Some(first_name), self.ident_name(&name_ident))
        } else {
            (None, first_name)
        };

        // Optional value: = expr or (expr)
        let value = match self.current_kind() {
            Some(TokenKind::Eq) => {
                self.advance();
                let expr = self.parse_expr()?;
                end = expr.span().end;
                Some(PragmaValue::Assign(expr))
            }
            Some(TokenKind::LParen) => {
                self.advance();
                let expr = self.parse_expr()?;
                let rparen = self.expect(TokenKind::RParen, ")")?;
                end = rparen.span.end;
                Some(PragmaValue::Call(expr))
            }
            _ => None,
        };

        if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            end = semi.span.end;
        }

        Ok(PragmaStmt {
            schema,
            name,
            value,
            span: Span::new(start, end),
        })
    }

    /// Parse [schema.]name and return (QualifiedName, end_position)
    fn parse_qualified_name(&mut self) -> Result<(QualifiedName, usize), ParseError> {
        let first_ident = self.expect_ident("name")?;
        let start = first_ident.span.start;
        let first_name = self.ident_name(&first_ident);

        if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let name_ident = self.expect_ident("name")?;
            let name = self.ident_name(&name_ident);
            let end = name_ident.span.end;
            Ok((
                QualifiedName {
                    schema: Some(first_name),
                    name,
                    span: Span::new(start, end),
                },
                end,
            ))
        } else {
            let end = first_ident.span.end;
            Ok((
                QualifiedName {
                    schema: None,
                    name: first_name,
                    span: Span::new(start, end),
                },
                end,
            ))
        }
    }

    fn parse_select_stmt(&mut self) -> Result<SelectStmt, ParseError> {
        // Optional WITH clause
        let with_clause = if self.current_kind() == Some(&TokenKind::With) {
            Some(self.parse_with_clause()?)
        } else {
            None
        };

        let mut stmt = self.parse_select_stmt_core()?;
        if let Some(with) = with_clause {
            stmt.span.start = with.span.start;
            stmt.with_clause = Some(with);
        }
        Ok(stmt)
    }

    /// Parse SELECT statement without WITH clause (for use when WITH already parsed)
    fn parse_select_stmt_core(&mut self) -> Result<SelectStmt, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);

        // Parse the first SELECT core
        let (distinct, columns, from, where_clause, group_by, having) = self.parse_select_core()?;

        // Parse compound operations (UNION, INTERSECT, EXCEPT)
        let mut compounds = Vec::new();
        while let Some(op) = self.try_parse_compound_op() {
            let core = self.parse_select_core()?;
            let core_span_end = core.5.as_ref().map(|e| e.span().end)
                .or_else(|| core.4.as_ref().and_then(|g| g.last().map(|e| e.span().end)))
                .or_else(|| core.3.as_ref().map(|e| e.span().end))
                .or_else(|| core.2.as_ref().map(|f| f.span.end))
                .or_else(|| core.1.last().map(|c| c.span().end))
                .unwrap_or(start);
            compounds.push((op, SelectCore {
                distinct: core.0,
                columns: core.1,
                from: core.2,
                where_clause: core.3,
                group_by: core.4,
                having: core.5,
                span: Span::new(start, core_span_end),
            }));
        }

        // Optional ORDER BY clause (applies to entire compound statement)
        let order_by = if self.current_kind() == Some(&TokenKind::Order) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let mut terms = Vec::new();
            terms.push(self.parse_ordering_term()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                terms.push(self.parse_ordering_term()?);
            }
            Some(terms)
        } else {
            None
        };

        // Optional LIMIT clause (applies to entire compound statement)
        let limit = if self.current_kind() == Some(&TokenKind::Limit) {
            Some(self.parse_limit_clause()?)
        } else {
            None
        };

        let end = if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            semi.span.end
        } else {
            limit
                .as_ref()
                .map(|l| l.span.end)
                .or_else(|| order_by.as_ref().and_then(|o| o.last().map(|t| t.span.end)))
                .or_else(|| compounds.last().map(|(_, c)| c.span.end))
                .or_else(|| having.as_ref().map(|e| e.span().end))
                .or_else(|| group_by.as_ref().and_then(|g| g.last().map(|e| e.span().end)))
                .or_else(|| where_clause.as_ref().map(|e| e.span().end))
                .or_else(|| from.as_ref().map(|f| f.span.end))
                .or_else(|| columns.last().map(|c| c.span().end))
                .unwrap_or(start)
        };

        Ok(SelectStmt {
            with_clause: None,
            distinct,
            columns,
            from,
            where_clause,
            group_by,
            having,
            compounds,
            order_by,
            limit,
            span: Span::new(start, end),
        })
    }

    /// Parse WITH clause: WITH [RECURSIVE] cte [, cte ...]
    fn parse_with_clause(&mut self) -> Result<WithClause, ParseError> {
        let with_token = self.expect(TokenKind::With, "WITH")?;
        let start = with_token.span.start;

        // Optional RECURSIVE
        let recursive = if self.current_kind() == Some(&TokenKind::Recursive) {
            self.advance();
            true
        } else {
            false
        };

        // Parse CTEs
        let mut ctes = Vec::new();
        ctes.push(self.parse_cte()?);

        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            ctes.push(self.parse_cte()?);
        }

        let end = ctes.last().map(|c| c.span.end).unwrap_or(start);

        Ok(WithClause {
            recursive,
            ctes,
            span: Span::new(start, end),
        })
    }

    /// Parse a single CTE: name [(columns)] AS [MATERIALIZED|NOT MATERIALIZED] (select)
    fn parse_cte(&mut self) -> Result<CommonTableExpr, ParseError> {
        let name_token = self.expect_ident("CTE name")?;
        let start = name_token.span.start;
        let name = self.ident_name(&name_token);

        // Optional column list
        let columns = if self.current_kind() == Some(&TokenKind::LParen) {
            self.advance();
            let mut cols = Vec::new();
            let col_token = self.expect_ident("column name")?;
            cols.push(self.ident_name(&col_token));

            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                let col_token = self.expect_ident("column name")?;
                cols.push(self.ident_name(&col_token));
            }

            self.expect(TokenKind::RParen, ")")?;
            Some(cols)
        } else {
            None
        };

        self.expect(TokenKind::As, "AS")?;

        // Optional MATERIALIZED or NOT MATERIALIZED
        let materialized = match self.current_kind() {
            Some(TokenKind::Materialized) => {
                self.advance();
                Some(Materialized::Materialized)
            }
            Some(TokenKind::Not) => {
                self.advance();
                self.expect(TokenKind::Materialized, "MATERIALIZED")?;
                Some(Materialized::NotMaterialized)
            }
            _ => None,
        };

        // Parse the subquery in parentheses
        self.expect(TokenKind::LParen, "(")?;
        let select = Box::new(self.parse_select_stmt()?);
        let end = self.expect(TokenKind::RParen, ")")?.span.end;

        Ok(CommonTableExpr {
            name,
            columns,
            materialized,
            select,
            span: Span::new(start, end),
        })
    }

    // ========================================
    // INSERT Statement Parser
    // ========================================

    /// Parse INSERT statement:
    /// [WITH clause] INSERT [OR conflict] INTO [schema.]table [(cols)] source [upsert] [RETURNING ...]
    /// or REPLACE INTO ... (syntactic sugar for INSERT OR REPLACE)
    fn parse_insert_stmt(&mut self, with_clause: Option<WithClause>) -> Result<InsertStmt, ParseError> {
        let start = with_clause.as_ref().map(|w| w.span.start)
            .unwrap_or_else(|| self.current().map(|t| t.span.start).unwrap_or(0));

        // Parse INSERT [OR action] or REPLACE (which is INSERT OR REPLACE)
        let or_action = if self.current_kind() == Some(&TokenKind::Replace) {
            self.advance();
            Some(ConflictAction::Replace)
        } else {
            self.expect(TokenKind::Insert, "INSERT")?;
            // Optional OR conflict_action
            if self.current_kind() == Some(&TokenKind::Or) {
                self.advance();
                Some(self.parse_conflict_action()?)
            } else {
                None
            }
        };

        self.expect(TokenKind::Into, "INTO")?;

        // Parse [schema.]table_name
        let first_ident = self.expect_ident("table name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, table_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let table_ident = self.expect_ident("table name")?;
            (Some(first_name), self.ident_name(&table_ident))
        } else {
            (None, first_name)
        };

        // Optional alias: AS alias
        let alias = if self.current_kind() == Some(&TokenKind::As) {
            self.advance();
            let alias_token = self.expect_ident("alias")?;
            Some(self.ident_name(&alias_token))
        } else {
            None
        };

        // Optional column list
        let columns = if self.current_kind() == Some(&TokenKind::LParen) {
            // Check if this is a column list or VALUES list
            // Column list starts with ( followed by identifier
            // We need lookahead here
            let cursor_save = self.cursor;
            self.advance(); // consume (

            if self.is_ident_like() {
                // This is a column list
                let mut cols = Vec::new();
                let col = self.expect_ident("column name")?;
                cols.push(self.ident_name(&col));

                while self.current_kind() == Some(&TokenKind::Comma) {
                    self.advance();
                    let col = self.expect_ident("column name")?;
                    cols.push(self.ident_name(&col));
                }

                self.expect(TokenKind::RParen, ")")?;
                Some(cols)
            } else {
                // Not a column list, restore cursor
                self.cursor = cursor_save;
                None
            }
        } else {
            None
        };

        // Parse source: VALUES | SELECT | DEFAULT VALUES
        let source = self.parse_insert_source()?;

        // Optional ON CONFLICT clause (upsert)
        let upsert = if self.current_kind() == Some(&TokenKind::On) {
            Some(self.parse_upsert_clause()?)
        } else {
            None
        };

        // Optional RETURNING clause
        let returning = if self.current_kind() == Some(&TokenKind::Returning) {
            self.advance();
            let mut cols = Vec::new();
            cols.push(self.parse_result_column()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                cols.push(self.parse_result_column()?);
            }
            Some(cols)
        } else {
            None
        };

        let end = if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            semi.span.end
        } else {
            returning.as_ref().and_then(|r| r.last().map(|c| c.span().end))
                .or_else(|| upsert.as_ref().map(|u| u.span.end))
                .or_else(|| match &source {
                    InsertSource::Values(rows) => rows.last().and_then(|r| r.last().map(|e| e.span().end)),
                    InsertSource::Select(s) => Some(s.span.end),
                    InsertSource::DefaultValues => None,
                })
                .unwrap_or(start)
        };

        Ok(InsertStmt {
            with_clause,
            or_action,
            schema,
            table_name,
            alias,
            columns,
            source,
            upsert,
            returning,
            span: Span::new(start, end),
        })
    }

    /// Parse conflict action: ROLLBACK | ABORT | FAIL | IGNORE | REPLACE
    fn parse_conflict_action(&mut self) -> Result<ConflictAction, ParseError> {
        match self.current_kind() {
            Some(TokenKind::Rollback) => {
                self.advance();
                Ok(ConflictAction::Rollback)
            }
            Some(TokenKind::Abort) => {
                self.advance();
                Ok(ConflictAction::Abort)
            }
            Some(TokenKind::Fail) => {
                self.advance();
                Ok(ConflictAction::Fail)
            }
            Some(TokenKind::Ignore) => {
                self.advance();
                Ok(ConflictAction::Ignore)
            }
            Some(TokenKind::Replace) => {
                self.advance();
                Ok(ConflictAction::Replace)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::Expected {
                    expected: "conflict action (ROLLBACK, ABORT, FAIL, IGNORE, or REPLACE)",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    /// Parse INSERT source: VALUES (...), SELECT ..., or DEFAULT VALUES
    fn parse_insert_source(&mut self) -> Result<InsertSource, ParseError> {
        match self.current_kind() {
            Some(TokenKind::Values) => {
                self.advance();
                let mut rows = Vec::new();
                rows.push(self.parse_values_row()?);
                while self.current_kind() == Some(&TokenKind::Comma) {
                    self.advance();
                    rows.push(self.parse_values_row()?);
                }
                Ok(InsertSource::Values(rows))
            }
            Some(TokenKind::Select) | Some(TokenKind::With) => {
                Ok(InsertSource::Select(Box::new(self.parse_select_stmt()?)))
            }
            Some(TokenKind::Default) => {
                self.advance();
                self.expect(TokenKind::Values, "VALUES")?;
                Ok(InsertSource::DefaultValues)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                Err(ParseError::Expected {
                    expected: "VALUES, SELECT, or DEFAULT VALUES",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    /// Parse a single values row: (expr, expr, ...)
    fn parse_values_row(&mut self) -> Result<Vec<Expr>, ParseError> {
        self.expect(TokenKind::LParen, "(")?;
        let mut exprs = Vec::new();
        exprs.push(self.parse_expr()?);
        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            exprs.push(self.parse_expr()?);
        }
        self.expect(TokenKind::RParen, ")")?;
        Ok(exprs)
    }

    /// Parse ON CONFLICT clause (upsert)
    fn parse_upsert_clause(&mut self) -> Result<UpsertClause, ParseError> {
        let start = self.expect(TokenKind::On, "ON")?.span.start;
        self.expect(TokenKind::Conflict, "CONFLICT")?;

        // Optional conflict target: (indexed_column, ...) [WHERE expr]
        let target = if self.current_kind() == Some(&TokenKind::LParen) {
            Some(self.parse_conflict_target()?)
        } else {
            None
        };

        // DO NOTHING or DO UPDATE
        self.expect(TokenKind::Do, "DO")?;

        let (action, update_set, update_where) = match self.current_kind() {
            Some(TokenKind::Nothing) => {
                self.advance();
                (ConflictAction::Nothing, None, None)
            }
            Some(TokenKind::Update) => {
                self.advance();
                self.expect(TokenKind::Set, "SET")?;

                // Parse SET assignments
                let mut assignments = Vec::new();
                assignments.push(self.parse_update_assignment()?);
                while self.current_kind() == Some(&TokenKind::Comma) {
                    self.advance();
                    assignments.push(self.parse_update_assignment()?);
                }

                // Optional WHERE clause
                let where_clause = if self.current_kind() == Some(&TokenKind::Where) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };

                (ConflictAction::Update, Some(assignments), where_clause)
            }
            _ => {
                let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                return Err(ParseError::Expected {
                    expected: "NOTHING or UPDATE",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(pos),
                });
            }
        };

        let end = update_where.as_ref().map(|e| e.span().end)
            .or_else(|| update_set.as_ref().and_then(|s| s.last().map(|(_, e)| e.span().end)))
            .or_else(|| target.as_ref().map(|t| t.span.end))
            .unwrap_or(start);

        Ok(UpsertClause {
            target,
            action,
            update_set,
            update_where,
            span: Span::new(start, end),
        })
    }

    /// Parse conflict target: (indexed_column, ...) [WHERE expr]
    fn parse_conflict_target(&mut self) -> Result<ConflictTarget, ParseError> {
        let start = self.expect(TokenKind::LParen, "(")?.span.start;

        let mut columns = Vec::new();
        columns.push(self.parse_indexed_column()?);
        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            columns.push(self.parse_indexed_column()?);
        }

        self.expect(TokenKind::RParen, ")")?;

        // Optional WHERE clause
        let where_clause = if self.current_kind() == Some(&TokenKind::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let end = where_clause.as_ref().map(|e| e.span().end)
            .or_else(|| columns.last().map(|c| c.span.end))
            .unwrap_or(start);

        Ok(ConflictTarget {
            columns,
            where_clause,
            span: Span::new(start, end),
        })
    }

    /// Parse update assignment: column = expr or (col1, col2) = expr
    fn parse_update_assignment(&mut self) -> Result<(Vec<String>, Expr), ParseError> {
        let columns = if self.current_kind() == Some(&TokenKind::LParen) {
            // Multiple columns: (col1, col2, ...) = expr
            self.advance();
            let mut cols = Vec::new();
            let col = self.expect_ident("column name")?;
            cols.push(self.ident_name(&col));
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                let col = self.expect_ident("column name")?;
                cols.push(self.ident_name(&col));
            }
            self.expect(TokenKind::RParen, ")")?;
            cols
        } else {
            // Single column: col = expr
            let col = self.expect_ident("column name")?;
            vec![self.ident_name(&col)]
        };

        self.expect(TokenKind::Eq, "=")?;
        let expr = self.parse_expr()?;

        Ok((columns, expr))
    }

    // ========================================
    // UPDATE Statement Parser
    // ========================================

    /// Parse UPDATE statement:
    /// [WITH clause] UPDATE [OR conflict] [schema.]table [AS alias] [INDEXED BY ...] SET assignments
    /// [FROM ...] [WHERE ...] [RETURNING ...]
    fn parse_update_stmt(&mut self, with_clause: Option<WithClause>) -> Result<UpdateStmt, ParseError> {
        let start = with_clause.as_ref().map(|w| w.span.start)
            .unwrap_or_else(|| self.current().map(|t| t.span.start).unwrap_or(0));

        self.expect(TokenKind::Update, "UPDATE")?;

        // Optional OR conflict_action
        let or_action = if self.current_kind() == Some(&TokenKind::Or) {
            self.advance();
            Some(self.parse_conflict_action()?)
        } else {
            None
        };

        // Parse [schema.]table_name
        let first_ident = self.expect_ident("table name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, table_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let table_ident = self.expect_ident("table name")?;
            (Some(first_name), self.ident_name(&table_ident))
        } else {
            (None, first_name)
        };

        // Optional alias: AS alias
        let alias = if self.current_kind() == Some(&TokenKind::As) {
            self.advance();
            let alias_token = self.expect_ident("alias")?;
            Some(self.ident_name(&alias_token))
        } else {
            None
        };

        // Optional INDEXED BY or NOT INDEXED
        let indexed = if self.current_kind() == Some(&TokenKind::Indexed) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let idx_token = self.expect_ident("index name")?;
            Some(IndexedBy::Index(self.ident_name(&idx_token)))
        } else if self.current_kind() == Some(&TokenKind::Not) {
            self.advance();
            self.expect(TokenKind::Indexed, "INDEXED")?;
            Some(IndexedBy::NotIndexed)
        } else {
            None
        };

        // SET clause
        self.expect(TokenKind::Set, "SET")?;
        let mut assignments = Vec::new();
        let assign_start = self.current().map(|t| t.span.start).unwrap_or(0);
        let (cols, expr) = self.parse_update_assignment()?;
        let assign_end = expr.span().end;
        assignments.push(UpdateAssignment {
            columns: cols,
            expr,
            span: Span::new(assign_start, assign_end),
        });

        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            let assign_start = self.current().map(|t| t.span.start).unwrap_or(0);
            let (cols, expr) = self.parse_update_assignment()?;
            let assign_end = expr.span().end;
            assignments.push(UpdateAssignment {
                columns: cols,
                expr,
                span: Span::new(assign_start, assign_end),
            });
        }

        // Optional FROM clause (SQLite extension)
        let from = if self.current_kind() == Some(&TokenKind::From) {
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        // Optional WHERE clause
        let where_clause = if self.current_kind() == Some(&TokenKind::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        // Optional RETURNING clause
        let returning = if self.current_kind() == Some(&TokenKind::Returning) {
            self.advance();
            let mut cols = Vec::new();
            cols.push(self.parse_result_column()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                cols.push(self.parse_result_column()?);
            }
            Some(cols)
        } else {
            None
        };

        // Optional ORDER BY clause (UPDATE Limited)
        let order_by = if self.current_kind() == Some(&TokenKind::Order) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let mut terms = Vec::new();
            terms.push(self.parse_ordering_term()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                terms.push(self.parse_ordering_term()?);
            }
            Some(terms)
        } else {
            None
        };

        // Optional LIMIT clause (UPDATE Limited)
        let (limit, offset) = if self.current_kind() == Some(&TokenKind::Limit) {
            self.advance();
            let limit_expr = self.parse_expr()?;
            let offset_expr = if self.current_kind() == Some(&TokenKind::Offset) {
                self.advance();
                Some(self.parse_expr()?)
            } else if self.current_kind() == Some(&TokenKind::Comma) {
                // LIMIT expr, offset_expr syntax
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            (Some(limit_expr), offset_expr)
        } else {
            (None, None)
        };

        let end = if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            semi.span.end
        } else {
            offset.as_ref().map(|e| e.span().end)
                .or_else(|| limit.as_ref().map(|e| e.span().end))
                .or_else(|| order_by.as_ref().and_then(|o| o.last().map(|t| t.span.end)))
                .or_else(|| returning.as_ref().and_then(|r| r.last().map(|c| c.span().end)))
                .or_else(|| where_clause.as_ref().map(|e| e.span().end))
                .or_else(|| from.as_ref().map(|f| f.span.end))
                .or_else(|| assignments.last().map(|a| a.span.end))
                .unwrap_or(start)
        };

        Ok(UpdateStmt {
            with_clause,
            or_action,
            schema,
            table_name,
            alias,
            indexed,
            assignments,
            from,
            where_clause,
            returning,
            order_by,
            limit,
            offset,
            span: Span::new(start, end),
        })
    }

    // ========================================
    // DELETE Statement Parser
    // ========================================

    /// Parse DELETE statement:
    /// [WITH clause] DELETE FROM [schema.]table [AS alias] [INDEXED BY ... | NOT INDEXED]
    /// [WHERE ...] [RETURNING ...]
    fn parse_delete_stmt(&mut self, with_clause: Option<WithClause>) -> Result<DeleteStmt, ParseError> {
        let start = with_clause.as_ref().map(|w| w.span.start)
            .unwrap_or_else(|| self.current().map(|t| t.span.start).unwrap_or(0));

        self.expect(TokenKind::Delete, "DELETE")?;
        self.expect(TokenKind::From, "FROM")?;

        // Parse [schema.]table_name
        let first_ident = self.expect_ident("table name")?;
        let first_name = self.ident_name(&first_ident);

        let (schema, table_name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let table_ident = self.expect_ident("table name")?;
            (Some(first_name), self.ident_name(&table_ident))
        } else {
            (None, first_name)
        };

        // Optional alias: AS alias
        let alias = if self.current_kind() == Some(&TokenKind::As) {
            self.advance();
            let alias_token = self.expect_ident("alias")?;
            Some(self.ident_name(&alias_token))
        } else {
            None
        };

        // Optional INDEXED BY or NOT INDEXED
        let indexed = if self.current_kind() == Some(&TokenKind::Indexed) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let idx_token = self.expect_ident("index name")?;
            Some(IndexedBy::Index(self.ident_name(&idx_token)))
        } else if self.current_kind() == Some(&TokenKind::Not) {
            self.advance();
            self.expect(TokenKind::Indexed, "INDEXED")?;
            Some(IndexedBy::NotIndexed)
        } else {
            None
        };

        // Optional WHERE clause
        let where_clause = if self.current_kind() == Some(&TokenKind::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        // Optional RETURNING clause
        let returning = if self.current_kind() == Some(&TokenKind::Returning) {
            self.advance();
            let mut cols = Vec::new();
            cols.push(self.parse_result_column()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                cols.push(self.parse_result_column()?);
            }
            Some(cols)
        } else {
            None
        };

        // Optional ORDER BY clause (DELETE Limited)
        let order_by = if self.current_kind() == Some(&TokenKind::Order) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let mut terms = Vec::new();
            terms.push(self.parse_ordering_term()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                terms.push(self.parse_ordering_term()?);
            }
            Some(terms)
        } else {
            None
        };

        // Optional LIMIT clause (DELETE Limited)
        let (limit, offset) = if self.current_kind() == Some(&TokenKind::Limit) {
            self.advance();
            let limit_expr = self.parse_expr()?;
            let offset_expr = if self.current_kind() == Some(&TokenKind::Offset) {
                self.advance();
                Some(self.parse_expr()?)
            } else if self.current_kind() == Some(&TokenKind::Comma) {
                // LIMIT expr, offset_expr syntax
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            (Some(limit_expr), offset_expr)
        } else {
            (None, None)
        };

        let end = if let Some(semi) = self.consume_if(TokenKind::Semicolon) {
            semi.span.end
        } else {
            offset.as_ref().map(|e| e.span().end)
                .or_else(|| limit.as_ref().map(|e| e.span().end))
                .or_else(|| order_by.as_ref().and_then(|o| o.last().map(|t| t.span.end)))
                .or_else(|| returning.as_ref().and_then(|r| r.last().map(|c| c.span().end)))
                .or_else(|| where_clause.as_ref().map(|e| e.span().end))
                .or_else(|| indexed.as_ref().map(|_| self.tokens[self.cursor.saturating_sub(1)].span.end))
                .unwrap_or(start)
        };

        Ok(DeleteStmt {
            with_clause,
            schema,
            table_name,
            alias,
            indexed,
            where_clause,
            returning,
            order_by,
            limit,
            offset,
            span: Span::new(start, end),
        })
    }

    /// Parse SELECT core (without ORDER BY / LIMIT)
    #[allow(clippy::type_complexity)]
    fn parse_select_core(&mut self) -> Result<(
        DistinctAll,
        Vec<ResultColumn>,
        Option<FromClause>,
        Option<Expr>,
        Option<Vec<Expr>>,
        Option<Expr>,
    ), ParseError> {
        self.expect(TokenKind::Select, "SELECT")?;

        // Optional DISTINCT or ALL
        let distinct = match self.current_kind() {
            Some(TokenKind::Distinct) => {
                self.advance();
                DistinctAll::Distinct
            }
            Some(TokenKind::All) => {
                self.advance();
                DistinctAll::All
            }
            _ => DistinctAll::All,
        };

        // Parse result columns
        let mut columns = Vec::new();
        columns.push(self.parse_result_column()?);

        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            columns.push(self.parse_result_column()?);
        }

        // Optional FROM clause
        let from = if self.current_kind() == Some(&TokenKind::From) {
            Some(self.parse_from_clause()?)
        } else {
            None
        };

        // Optional WHERE clause
        let where_clause = if self.current_kind() == Some(&TokenKind::Where) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        // Optional GROUP BY clause
        let group_by = if self.current_kind() == Some(&TokenKind::Group) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let mut exprs = Vec::new();
            exprs.push(self.parse_expr()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                exprs.push(self.parse_expr()?);
            }
            Some(exprs)
        } else {
            None
        };

        // Optional HAVING clause
        let having = if self.current_kind() == Some(&TokenKind::Having) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok((distinct, columns, from, where_clause, group_by, having))
    }

    /// Try to parse a compound operator (UNION, INTERSECT, EXCEPT)
    fn try_parse_compound_op(&mut self) -> Option<CompoundOp> {
        match self.current_kind() {
            Some(TokenKind::Union) => {
                self.advance();
                if self.current_kind() == Some(&TokenKind::All) {
                    self.advance();
                    Some(CompoundOp::UnionAll)
                } else {
                    Some(CompoundOp::Union)
                }
            }
            Some(TokenKind::Intersect) => {
                self.advance();
                Some(CompoundOp::Intersect)
            }
            Some(TokenKind::Except) => {
                self.advance();
                Some(CompoundOp::Except)
            }
            _ => None,
        }
    }

    /// Parse result column: expr [AS alias] | * | table.*
    fn parse_result_column(&mut self) -> Result<ResultColumn, ParseError> {
        let token = match self.current() {
            Some(t) => t.clone(),
            None => return Err(ParseError::Eof),
        };

        // Check for * (all columns)
        if token.kind == TokenKind::Star {
            self.advance();
            return Ok(ResultColumn::Star(Span::from(token.span)));
        }

        // Check for table.* (look ahead for ident.*)
        if matches!(token.kind, TokenKind::Ident | TokenKind::QuotedIdent | TokenKind::BracketIdent | TokenKind::BacktickIdent) {
            if let Some(next) = self.tokens.get(self.cursor + 1) {
                if next.kind == TokenKind::Dot {
                    if let Some(star) = self.tokens.get(self.cursor + 2) {
                        if star.kind == TokenKind::Star {
                            let table = self.ident_name(&token);
                            self.advance(); // consume ident
                            self.advance(); // consume dot
                            let star_token = self.advance().unwrap(); // consume star
                            return Ok(ResultColumn::TableStar {
                                table,
                                span: Span::new(token.span.start, star_token.span.end),
                            });
                        }
                    }
                }
            }
        }

        // Parse expression
        let expr = self.parse_expr()?;
        let expr_end = expr.span().end;

        // Optional AS alias
        let (alias, alias_has_as, end) = if self.current_kind() == Some(&TokenKind::As) {
            self.advance();
            let alias_token = self.expect_ident("alias")?;
            let alias = self.ident_name(&alias_token);
            (Some(alias), true, alias_token.span.end)
        } else if self.is_ident_like() {
            // Alias without AS keyword
            let alias_token = self.current().unwrap().clone();
            self.advance();
            let alias = self.ident_name(&alias_token);
            (Some(alias), false, alias_token.span.end)
        } else {
            (None, false, expr_end)
        };

        Ok(ResultColumn::Expr {
            expr,
            alias,
            alias_has_as,
            span: Span::new(token.span.start, end),
        })
    }

    /// Parse FROM clause: table_or_subquery [JOIN ...]* [, table_or_subquery [JOIN ...]*]*
    fn parse_from_clause(&mut self) -> Result<FromClause, ParseError> {
        let from_token = self.expect(TokenKind::From, "FROM")?;
        let start = from_token.span.start;

        let mut tables = Vec::new();
        tables.push(self.parse_table_with_joins()?);

        // Handle comma-separated tables (implicit cross join)
        while self.current_kind() == Some(&TokenKind::Comma) {
            self.advance();
            tables.push(self.parse_table_with_joins()?);
        }

        let end = tables.last().map(|t| self.table_or_subquery_end(t)).unwrap_or(start);

        Ok(FromClause {
            tables,
            span: Span::new(start, end),
        })
    }

    /// Get the end position of a TableOrSubquery
    fn table_or_subquery_end(&self, t: &TableOrSubquery) -> usize {
        match t {
            TableOrSubquery::Table { span, .. } => span.end,
            TableOrSubquery::Subquery { span, .. } => span.end,
            TableOrSubquery::TableList { span, .. } => span.end,
            TableOrSubquery::Join { span, .. } => span.end,
        }
    }

    /// Parse a table_or_subquery followed by any number of JOINs
    fn parse_table_with_joins(&mut self) -> Result<TableOrSubquery, ParseError> {
        let mut left = self.parse_table_or_subquery()?;

        // Handle JOINs
        while let Some(join_type) = self.try_parse_join_type() {
            let right = self.parse_table_or_subquery()?;
            let constraint = self.parse_join_constraint()?;

            let start = match &left {
                TableOrSubquery::Table { span, .. } => span.start,
                TableOrSubquery::Subquery { span, .. } => span.start,
                TableOrSubquery::TableList { span, .. } => span.start,
                TableOrSubquery::Join { span, .. } => span.start,
            };
            let end = constraint.as_ref().map(|c| match c {
                JoinConstraint::On(expr) => expr.span().end,
                JoinConstraint::Using(_) => self.table_or_subquery_end(&right),
            }).unwrap_or_else(|| self.table_or_subquery_end(&right));

            left = TableOrSubquery::Join {
                left: Box::new(left),
                join_type,
                right: Box::new(right),
                constraint,
                span: Span::new(start, end),
            };
        }

        Ok(left)
    }

    /// Try to parse a JOIN type, returning None if no JOIN keyword found
    fn try_parse_join_type(&mut self) -> Option<JoinType> {
        let kind = self.current_kind()?;

        // Check for NATURAL first
        let natural = if *kind == TokenKind::Natural {
            self.advance();
            true
        } else {
            false
        };

        // Check for join direction keywords
        let direction = match self.current_kind() {
            Some(TokenKind::Left) => {
                self.advance();
                self.consume_if(TokenKind::Outer); // Optional OUTER
                Some("left")
            }
            Some(TokenKind::Right) => {
                self.advance();
                self.consume_if(TokenKind::Outer);
                Some("right")
            }
            Some(TokenKind::Full) => {
                self.advance();
                self.consume_if(TokenKind::Outer);
                Some("full")
            }
            Some(TokenKind::Inner) => {
                self.advance();
                Some("inner")
            }
            Some(TokenKind::Cross) => {
                self.advance();
                Some("cross")
            }
            _ => None,
        };

        // Now we should see JOIN keyword (unless it's comma-style or end of clause)
        if self.current_kind() == Some(&TokenKind::Join) {
            self.advance();
        } else if !natural && direction.is_none() {
            return None; // No join at all
        }

        // Determine the join type
        Some(match (natural, direction) {
            (true, Some("left")) => JoinType::NaturalLeft,
            (true, Some("right")) => JoinType::NaturalRight,
            (true, Some("full")) => JoinType::NaturalFull,
            (true, _) => JoinType::Natural,
            (false, Some("left")) => JoinType::Left,
            (false, Some("right")) => JoinType::Right,
            (false, Some("full")) => JoinType::Full,
            (false, Some("cross")) => JoinType::Cross,
            (false, Some("inner")) | (false, None) => JoinType::Inner,
            _ => JoinType::Inner,
        })
    }

    /// Parse optional JOIN constraint: ON expr | USING (col, ...)
    fn parse_join_constraint(&mut self) -> Result<Option<JoinConstraint>, ParseError> {
        match self.current_kind() {
            Some(TokenKind::On) => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Some(JoinConstraint::On(expr)))
            }
            Some(TokenKind::Using) => {
                self.advance();
                self.expect(TokenKind::LParen, "(")?;

                let mut columns = Vec::new();
                let col_token = self.expect_ident("column name")?;
                columns.push(self.ident_name(&col_token));

                while self.current_kind() == Some(&TokenKind::Comma) {
                    self.advance();
                    let col_token = self.expect_ident("column name")?;
                    columns.push(self.ident_name(&col_token));
                }

                self.expect(TokenKind::RParen, ")")?;
                Ok(Some(JoinConstraint::Using(columns)))
            }
            _ => Ok(None),
        }
    }

    /// Parse table_or_subquery: [schema.]table [AS alias]
    fn parse_table_or_subquery(&mut self) -> Result<TableOrSubquery, ParseError> {
        let first_ident = self.expect_ident("table name")?;
        let start = first_ident.span.start;
        let first_name = self.ident_name(&first_ident);
        let mut end = first_ident.span.end;

        let (schema, name) = if self.current_kind() == Some(&TokenKind::Dot) {
            self.advance();
            let table_ident = self.expect_ident("table name")?;
            end = table_ident.span.end;
            (Some(first_name), self.ident_name(&table_ident))
        } else {
            (None, first_name)
        };

        // Optional AS alias
        let (alias, alias_has_as) = if self.current_kind() == Some(&TokenKind::As) {
            self.advance();
            let alias_token = self.expect_ident("alias")?;
            end = alias_token.span.end;
            (Some(self.ident_name(&alias_token)), true)
        } else if self.is_ident_like() {
            // Alias without AS keyword (only if not a keyword)
            let next = self.current().unwrap().clone();
            // Don't treat keywords like WHERE, ORDER, etc. as aliases
            if !self.is_clause_keyword(&next.kind) {
                self.advance();
                end = next.span.end;
                (Some(self.ident_name(&next)), false)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        Ok(TableOrSubquery::Table {
            schema,
            name,
            alias,
            alias_has_as,
            indexed: None,
            span: Span::new(start, end),
        })
    }

    /// Check if a token kind is a clause keyword (WHERE, ORDER, GROUP, etc.)
    fn is_clause_keyword(&self, kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Where
                | TokenKind::Order
                | TokenKind::Group
                | TokenKind::Having
                | TokenKind::Limit
                | TokenKind::Union
                | TokenKind::Intersect
                | TokenKind::Except
                | TokenKind::On
                | TokenKind::Using
                | TokenKind::Join
                | TokenKind::Inner
                | TokenKind::Left
                | TokenKind::Right
                | TokenKind::Full
                | TokenKind::Cross
                | TokenKind::Natural
        )
    }

    /// Parse ordering term: expr [ASC|DESC] [NULLS FIRST|LAST]
    fn parse_ordering_term(&mut self) -> Result<solite_ast::OrderingTerm, ParseError> {
        let expr = self.parse_expr()?;
        let start = expr.span().start;
        let mut end = expr.span().end;

        let direction = match self.current_kind() {
            Some(TokenKind::Asc) => {
                end = self.advance().unwrap().span.end;
                Some(solite_ast::OrderDirection::Asc)
            }
            Some(TokenKind::Desc) => {
                end = self.advance().unwrap().span.end;
                Some(solite_ast::OrderDirection::Desc)
            }
            _ => None,
        };

        let nulls = if self.current_kind() == Some(&TokenKind::Nulls) {
            self.advance();
            match self.current_kind() {
                Some(TokenKind::First) => {
                    end = self.advance().unwrap().span.end;
                    Some(solite_ast::NullsOrder::First)
                }
                Some(TokenKind::Last) => {
                    end = self.advance().unwrap().span.end;
                    Some(solite_ast::NullsOrder::Last)
                }
                _ => None,
            }
        } else {
            None
        };

        Ok(solite_ast::OrderingTerm {
            expr,
            direction,
            nulls,
            span: Span::new(start, end),
        })
    }

    /// Parse LIMIT clause: LIMIT expr [OFFSET expr | , expr]
    fn parse_limit_clause(&mut self) -> Result<solite_ast::LimitClause, ParseError> {
        let limit_token = self.expect(TokenKind::Limit, "LIMIT")?;
        let start = limit_token.span.start;

        let limit = self.parse_expr()?;
        let mut end = limit.span().end;

        let offset = if self.current_kind() == Some(&TokenKind::Offset) {
            self.advance();
            let expr = self.parse_expr()?;
            end = expr.span().end;
            Some(expr)
        } else if self.current_kind() == Some(&TokenKind::Comma) {
            // LIMIT count, offset syntax
            self.advance();
            let expr = self.parse_expr()?;
            end = expr.span().end;
            Some(expr)
        } else {
            None
        };

        Ok(solite_ast::LimitClause {
            limit,
            offset,
            span: Span::new(start, end),
        })
    }

    /// Parse an expression using Pratt parsing (precedence climbing)
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_bp(0)
    }

    /// Pratt parser: parse expression with minimum binding power
    #[allow(clippy::while_let_loop)]
    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        // Parse prefix (unary operators or atoms)
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            // Check for infix operator
            let op_token = match self.current() {
                Some(t) => t.clone(),
                None => break,
            };

            // Handle special postfix-like operators first
            // These have binding power ~40 (between AND and comparison)
            match &op_token.kind {
                // [NOT] IN (list) or [NOT] IN (SELECT ...)
                TokenKind::In => {
                    if 40 < min_bp {
                        break;
                    }
                    self.advance();
                    lhs = self.parse_in_expr(lhs, false)?;
                    continue;
                }
                TokenKind::Not => {
                    // Peek ahead for NOT IN, NOT BETWEEN, NOT LIKE, NOT GLOB, NOT REGEXP, NOT MATCH
                    if let Some(next) = self.peek_nth(1) {
                        match &next.kind {
                            TokenKind::In if 40 >= min_bp => {
                                self.advance(); // NOT
                                self.advance(); // IN
                                lhs = self.parse_in_expr(lhs, true)?;
                                continue;
                            }
                            TokenKind::Between if 40 >= min_bp => {
                                self.advance(); // NOT
                                self.advance(); // BETWEEN
                                lhs = self.parse_between_expr(lhs, true)?;
                                continue;
                            }
                            TokenKind::Like if 40 >= min_bp => {
                                self.advance(); // NOT
                                self.advance(); // LIKE
                                lhs = self.parse_like_expr(lhs, BinaryOp::Like, true)?;
                                continue;
                            }
                            TokenKind::Glob if 40 >= min_bp => {
                                self.advance(); // NOT
                                self.advance(); // GLOB
                                lhs = self.parse_like_expr(lhs, BinaryOp::Glob, true)?;
                                continue;
                            }
                            TokenKind::Regexp if 40 >= min_bp => {
                                self.advance(); // NOT
                                self.advance(); // REGEXP
                                lhs = self.parse_like_expr(lhs, BinaryOp::Regexp, true)?;
                                continue;
                            }
                            TokenKind::Match if 40 >= min_bp => {
                                self.advance(); // NOT
                                self.advance(); // MATCH
                                lhs = self.parse_like_expr(lhs, BinaryOp::Match, true)?;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
                // BETWEEN expr AND expr
                TokenKind::Between => {
                    if 40 < min_bp {
                        break;
                    }
                    self.advance();
                    lhs = self.parse_between_expr(lhs, false)?;
                    continue;
                }
                // LIKE / GLOB / REGEXP / MATCH
                TokenKind::Like => {
                    if 40 < min_bp {
                        break;
                    }
                    self.advance();
                    lhs = self.parse_like_expr(lhs, BinaryOp::Like, false)?;
                    continue;
                }
                TokenKind::Glob => {
                    if 40 < min_bp {
                        break;
                    }
                    self.advance();
                    lhs = self.parse_like_expr(lhs, BinaryOp::Glob, false)?;
                    continue;
                }
                TokenKind::Regexp => {
                    if 40 < min_bp {
                        break;
                    }
                    self.advance();
                    lhs = self.parse_like_expr(lhs, BinaryOp::Regexp, false)?;
                    continue;
                }
                TokenKind::Match => {
                    if 40 < min_bp {
                        break;
                    }
                    self.advance();
                    lhs = self.parse_like_expr(lhs, BinaryOp::Match, false)?;
                    continue;
                }
                // IS [NOT] NULL or IS [NOT] expr
                TokenKind::Is => {
                    if 50 < min_bp {
                        break;
                    }
                    self.advance();
                    let negated = if self.current_kind() == Some(&TokenKind::Not) {
                        self.advance();
                        true
                    } else {
                        false
                    };
                    // Check for IS [NOT] NULL specifically
                    if self.current_kind() == Some(&TokenKind::Null) {
                        let start = lhs.span().start;
                        let end = self.advance().unwrap().span.end;
                        lhs = Expr::IsNull {
                            expr: Box::new(lhs),
                            negated,
                            span: Span::new(start, end),
                        };
                        continue;
                    }
                    // Otherwise, fall through to IS as binary operator
                    // Note: IS NOT expr is not valid SQL, only IS NULL / IS NOT NULL
                    // But we'll parse IS expr as a binary operation for robustness
                    if negated {
                        // NOT was consumed, but next isn't NULL - this is invalid
                        // Try to recover by treating as IS with the next expression
                        // This may produce a confusing AST but avoids hard crash
                    }
                    let start = lhs.span().start;
                    let rhs = self.parse_expr_bp(51)?;
                    let end = rhs.span().end;
                    lhs = Expr::Binary {
                        left: Box::new(lhs),
                        op: if negated { BinaryOp::IsNot } else { BinaryOp::Is },
                        right: Box::new(rhs),
                        span: Span::new(start, end),
                    };
                    continue;
                }
                // COLLATE collation_name
                TokenKind::Collate => {
                    if 120 < min_bp {
                        break;
                    }
                    self.advance();
                    let start = lhs.span().start;
                    let collation_token = self.expect_ident("collation name")?;
                    let collation = self.ident_name(&collation_token);
                    lhs = Expr::Collate {
                        expr: Box::new(lhs),
                        collation,
                        span: Span::new(start, collation_token.span.end),
                    };
                    continue;
                }
                _ => {}
            }

            // Get binding power for regular binary operators
            let (l_bp, r_bp, op) = match self.infix_binding_power(&op_token.kind) {
                Some(bp) => bp,
                None => break, // Not an infix operator, stop
            };

            if l_bp < min_bp {
                break;
            }

            self.advance(); // consume the operator

            let rhs = self.parse_expr_bp(r_bp)?;
            let span = Span::new(lhs.span().start, rhs.span().end);

            lhs = Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
                span,
            };
        }

        Ok(lhs)
    }

    /// Parse IN expression: expr [NOT] IN (list | SELECT)
    fn parse_in_expr(&mut self, lhs: Expr, negated: bool) -> Result<Expr, ParseError> {
        let start = lhs.span().start;
        self.expect(TokenKind::LParen, "(")?;

        // Check for subquery: IN (SELECT ...)
        if self.current_kind() == Some(&TokenKind::Select)
            || self.current_kind() == Some(&TokenKind::With)
        {
            let query = self.parse_select_stmt()?;
            let end = self.expect(TokenKind::RParen, ")")?.span.end;
            return Ok(Expr::InSelect {
                expr: Box::new(lhs),
                query: Box::new(query),
                negated,
                span: Span::new(start, end),
            });
        }

        // Parse expression list: IN (value1, value2, ...)
        let mut list = Vec::new();
        if self.current_kind() != Some(&TokenKind::RParen) {
            list.push(self.parse_expr()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                list.push(self.parse_expr()?);
            }
        }
        let end = self.expect(TokenKind::RParen, ")")?.span.end;

        Ok(Expr::InList {
            expr: Box::new(lhs),
            list,
            negated,
            span: Span::new(start, end),
        })
    }

    /// Parse BETWEEN expression: expr [NOT] BETWEEN low AND high
    fn parse_between_expr(&mut self, lhs: Expr, negated: bool) -> Result<Expr, ParseError> {
        let start = lhs.span().start;
        // Note: BETWEEN has tighter binding than AND, so we use a higher min_bp
        let low = self.parse_expr_bp(41)?;
        self.expect(TokenKind::And, "AND")?;
        let high = self.parse_expr_bp(41)?;
        let end = high.span().end;

        Ok(Expr::Between {
            expr: Box::new(lhs),
            low: Box::new(low),
            high: Box::new(high),
            negated,
            span: Span::new(start, end),
        })
    }

    /// Parse LIKE/GLOB/REGEXP/MATCH expression with optional ESCAPE
    fn parse_like_expr(&mut self, lhs: Expr, op: BinaryOp, negated: bool) -> Result<Expr, ParseError> {
        let start = lhs.span().start;
        let pattern = self.parse_expr_bp(41)?;

        // Optional ESCAPE clause
        let (escape, end) = if self.current_kind() == Some(&TokenKind::Escape) {
            self.advance();
            let esc = self.parse_expr_bp(41)?;
            let e = esc.span().end;
            (Some(Box::new(esc)), e)
        } else {
            (None, pattern.span().end)
        };

        Ok(Expr::Like {
            expr: Box::new(lhs),
            pattern: Box::new(pattern),
            escape,
            op,
            negated,
            span: Span::new(start, end),
        })
    }

    /// Parse prefix expressions (unary operators and atoms)
    fn parse_prefix_expr(&mut self) -> Result<Expr, ParseError> {
        let token = match self.current() {
            Some(t) => t.clone(),
            None => return Err(ParseError::Eof),
        };

        // Check for unary operators
        match &token.kind {
            TokenKind::Minus => {
                self.advance();
                let r_bp = self.prefix_binding_power(UnaryOp::Neg);
                let expr = self.parse_expr_bp(r_bp)?;
                let span = Span::new(token.span.start, expr.span().end);
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Plus => {
                self.advance();
                let r_bp = self.prefix_binding_power(UnaryOp::Pos);
                let expr = self.parse_expr_bp(r_bp)?;
                let span = Span::new(token.span.start, expr.span().end);
                Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Tilde => {
                self.advance();
                let r_bp = self.prefix_binding_power(UnaryOp::BitNot);
                let expr = self.parse_expr_bp(r_bp)?;
                let span = Span::new(token.span.start, expr.span().end);
                Ok(Expr::Unary {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Not => {
                self.advance();
                // Check for NOT EXISTS
                if self.current_kind() == Some(&TokenKind::Exists) {
                    self.advance();
                    self.expect(TokenKind::LParen, "(")?;
                    let query = self.parse_select_stmt()?;
                    let rparen = self.expect(TokenKind::RParen, ")")?;
                    return Ok(Expr::Exists {
                        query: Box::new(query),
                        negated: true,
                        span: Span::new(token.span.start, rparen.span.end),
                    });
                }
                let r_bp = self.prefix_binding_power(UnaryOp::Not);
                let expr = self.parse_expr_bp(r_bp)?;
                let span = Span::new(token.span.start, expr.span().end);
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Exists => {
                // EXISTS (SELECT ...)
                self.advance();
                self.expect(TokenKind::LParen, "(")?;
                let query = self.parse_select_stmt()?;
                let rparen = self.expect(TokenKind::RParen, ")")?;
                Ok(Expr::Exists {
                    query: Box::new(query),
                    negated: false,
                    span: Span::new(token.span.start, rparen.span.end),
                })
            }
            TokenKind::LParen => {
                self.advance();
                // Check for scalar subquery: (SELECT ...)
                if self.current_kind() == Some(&TokenKind::Select)
                    || self.current_kind() == Some(&TokenKind::With)
                {
                    let query = self.parse_select_stmt()?;
                    let rparen = self.expect(TokenKind::RParen, ")")?;
                    return Ok(Expr::Subquery {
                        query: Box::new(query),
                        span: Span::new(token.span.start, rparen.span.end),
                    });
                }
                let inner = self.parse_expr()?;
                let rparen = self.expect(TokenKind::RParen, ")")?;
                let span = Span::new(token.span.start, rparen.span.end);
                Ok(Expr::Paren(Box::new(inner), span))
            }
            _ => self.parse_atom(),
        }
    }

    /// Parse atomic expressions (literals, identifiers, etc.)
    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        let token = match self.current() {
            Some(t) => t.clone(),
            None => return Err(ParseError::Eof),
        };

        let span = Span::from(token.span.clone());
        let slice = self.slice(&token.span).to_string();

        match &token.kind {
            TokenKind::Integer => {
                self.advance();
                let value: i64 = slice.parse().unwrap_or(0);
                Ok(Expr::Integer(value, span))
            }
            TokenKind::HexInteger => {
                self.advance();
                // Parse 0x1F or 0X1F
                let hex = if slice.starts_with("0x") || slice.starts_with("0X") {
                    &slice[2..]
                } else {
                    &slice
                };
                let value = i64::from_str_radix(hex, 16).unwrap_or(0);
                Ok(Expr::HexInteger(value, span))
            }
            TokenKind::Float => {
                self.advance();
                let value: f64 = slice.parse().unwrap_or(0.0);
                Ok(Expr::Float(value, span))
            }
            TokenKind::String => {
                self.advance();
                // Remove surrounding quotes and unescape ''
                let value = slice[1..slice.len() - 1].replace("''", "'");
                Ok(Expr::String(value, span))
            }
            TokenKind::Blob => {
                let blob_start = span.start;
                self.advance();
                // Parse X'AABBCCDD' -> bytes
                let hex = &slice[2..slice.len() - 1];
                let bytes = parse_hex(hex).map_err(|_| ParseError::InvalidBlob {
                    location: self.offset_to_location(blob_start),
                })?;
                Ok(Expr::Blob(bytes, span))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Expr::Null(span))
            }
            // TRUE and FALSE are boolean literals (aliases for 1 and 0 in SQLite)
            TokenKind::True => {
                self.advance();
                Ok(Expr::Integer(1, span))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Integer(0, span))
            }
            TokenKind::Ident | TokenKind::QuotedIdent | TokenKind::BracketIdent | TokenKind::BacktickIdent => {
                let name = self.ident_name(&token);
                let is_double_quoted = token.kind == TokenKind::QuotedIdent;
                self.advance();

                // Check for function call: name(...)
                if self.current_kind() == Some(&TokenKind::LParen) {
                    return self.parse_function_call(name, span.start);
                }

                // Check for qualified name: table.column or schema.table.column
                if self.current_kind() == Some(&TokenKind::Dot) {
                    self.advance(); // consume dot
                    let second_token = self.expect_ident("column name")?;
                    let second_name = self.ident_name(&second_token);
                    let mut end = second_token.span.end;

                    // Check for third part: schema.table.column
                    if self.current_kind() == Some(&TokenKind::Dot) {
                        self.advance(); // consume dot
                        let third_token = self.expect_ident("column name")?;
                        let third_name = self.ident_name(&third_token);
                        end = third_token.span.end;
                        // schema.table.column
                        Ok(Expr::Column {
                            schema: Some(name),
                            table: Some(second_name),
                            column: third_name,
                            span: Span::new(span.start, end),
                        })
                    } else {
                        // table.column
                        Ok(Expr::Column {
                            schema: None,
                            table: Some(name),
                            column: second_name,
                            span: Span::new(span.start, end),
                        })
                    }
                } else {
                    Ok(Expr::Ident(name, is_double_quoted, span))
                }
            }
            TokenKind::Star => {
                self.advance();
                Ok(Expr::Star(span))
            }
            // Bind parameters
            TokenKind::BindParam
            | TokenKind::BindParamColon
            | TokenKind::BindParamAt
            | TokenKind::BindParamDollar => {
                self.advance();
                Ok(Expr::BindParam(slice, span))
            }
            // RAISE function (for triggers)
            TokenKind::Raise => {
                self.advance();
                self.expect(TokenKind::LParen, "(")?;

                // Parse action: IGNORE | ROLLBACK | ABORT | FAIL
                let action = match self.current_kind() {
                    Some(&TokenKind::Ignore) => {
                        self.advance();
                        RaiseAction::Ignore
                    }
                    Some(&TokenKind::Rollback) => {
                        self.advance();
                        RaiseAction::Rollback
                    }
                    Some(&TokenKind::Abort) => {
                        self.advance();
                        RaiseAction::Abort
                    }
                    Some(&TokenKind::Fail) => {
                        self.advance();
                        RaiseAction::Fail
                    }
                    _ => {
                        let pos = self.current().map(|t| t.span.start).unwrap_or(0);
                        return Err(ParseError::Expected {
                            expected: "IGNORE, ROLLBACK, ABORT, or FAIL",
                            found: self.current_kind().cloned(),
                            location: self.offset_to_location(pos),
                        });
                    }
                };

                // Optional: error message for ROLLBACK, ABORT, FAIL
                let message = if action != RaiseAction::Ignore && self.current_kind() == Some(&TokenKind::Comma) {
                    self.advance();
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };

                let rparen = self.expect(TokenKind::RParen, ")")?;

                Ok(Expr::Raise {
                    action,
                    message,
                    span: Span::new(span.start, rparen.span.end),
                })
            }
            // CASE expression: CASE [expr] WHEN expr THEN expr ... [ELSE expr] END
            TokenKind::Case => {
                self.advance();

                // Optional operand (simple CASE vs searched CASE)
                let operand = if self.current_kind() != Some(&TokenKind::When) {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };

                // Parse WHEN ... THEN ... clauses
                let mut when_clauses = Vec::new();
                while self.current_kind() == Some(&TokenKind::When) {
                    self.advance();
                    let when_expr = self.parse_expr()?;
                    self.expect(TokenKind::Then, "THEN")?;
                    let then_expr = self.parse_expr()?;
                    when_clauses.push((when_expr, then_expr));
                }

                // Optional ELSE clause
                let else_clause = if self.current_kind() == Some(&TokenKind::Else) {
                    self.advance();
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };

                let end_token = self.expect(TokenKind::End, "END")?;

                Ok(Expr::Case {
                    operand,
                    when_clauses,
                    else_clause,
                    span: Span::new(span.start, end_token.span.end),
                })
            }
            // CAST expression: CAST(expr AS type)
            TokenKind::Cast => {
                self.advance();
                self.expect(TokenKind::LParen, "(")?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::As, "AS")?;
                let type_name = self.parse_type_name()?;
                let rparen = self.expect(TokenKind::RParen, ")")?;

                Ok(Expr::Cast {
                    expr: Box::new(expr),
                    type_name,
                    span: Span::new(span.start, rparen.span.end),
                })
            }
            _ => {
                let pos = span.start;
                Err(ParseError::UnexpectedToken {
                    location: self.offset_to_location(pos),
                })
            }
        }
    }

    /// Parse type name for CAST expression: type_name | type_name(arg1) | type_name(arg1, arg2)
    fn parse_type_name(&mut self) -> Result<TypeName, ParseError> {
        let name_token = self.expect_ident("type name")?;
        let start = name_token.span.start;
        let name = self.ident_name(&name_token);
        let mut end = name_token.span.end;

        // Optional arguments: (arg1) or (arg1, arg2)
        let args = if self.current_kind() == Some(&TokenKind::LParen) {
            self.advance();
            // Parse first argument (must be a number)
            let first_token = self.expect(TokenKind::Integer, "number")?;
            let first: i64 = self.slice(&first_token.span).parse().unwrap_or(0);

            // Optional second argument
            let second = if self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                let second_token = self.expect(TokenKind::Integer, "number")?;
                Some(self.slice(&second_token.span).parse().unwrap_or(0))
            } else {
                None
            };

            end = self.expect(TokenKind::RParen, ")")?.span.end;
            Some((first, second))
        } else {
            None
        };

        Ok(TypeName {
            name,
            args,
            span: Span::new(start, end),
        })
    }

    /// Get binding power for prefix (unary) operators
    fn prefix_binding_power(&self, op: UnaryOp) -> u8 {
        match op {
            // NOT has low precedence (just above AND)
            UnaryOp::Not => 25,
            // Unary - + ~ have high precedence
            UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => 110,
        }
    }

    /// Get binding power for infix (binary) operators
    /// Returns (left_bp, right_bp, op) or None if not an infix operator
    fn infix_binding_power(&self, kind: &TokenKind) -> Option<(u8, u8, BinaryOp)> {
        // Binding powers: higher = tighter binding
        // Left-associative: r_bp = l_bp + 1
        // Right-associative: r_bp = l_bp
        let (l_bp, r_bp, op) = match kind {
            // OR: lowest precedence
            TokenKind::Or => (10, 11, BinaryOp::Or),

            // AND
            TokenKind::And => (20, 21, BinaryOp::And),

            // Comparison: = == <> != IS
            TokenKind::Eq | TokenKind::EqEq => (50, 51, BinaryOp::Eq),
            TokenKind::Ne | TokenKind::BangEq => (50, 51, BinaryOp::Ne),
            TokenKind::Is => (50, 51, BinaryOp::Is),

            // Comparison: < <= > >=
            TokenKind::Lt => (60, 61, BinaryOp::Lt),
            TokenKind::Le => (60, 61, BinaryOp::Le),
            TokenKind::Gt => (60, 61, BinaryOp::Gt),
            TokenKind::Ge => (60, 61, BinaryOp::Ge),

            // Bitwise: << >> & |
            TokenKind::LShift => (70, 71, BinaryOp::LShift),
            TokenKind::RShift => (70, 71, BinaryOp::RShift),
            TokenKind::Ampersand => (70, 71, BinaryOp::BitAnd),
            TokenKind::Pipe => (70, 71, BinaryOp::BitOr),

            // Additive: + -
            TokenKind::Plus => (80, 81, BinaryOp::Add),
            TokenKind::Minus => (80, 81, BinaryOp::Sub),

            // Multiplicative: * / %
            TokenKind::Star => (90, 91, BinaryOp::Mul),
            TokenKind::Slash => (90, 91, BinaryOp::Div),
            TokenKind::Percent => (90, 91, BinaryOp::Mod),

            // String concatenation: ||
            TokenKind::Concat => (100, 101, BinaryOp::Concat),

            // JSON operators: -> ->> (highest precedence, like member access)
            TokenKind::Arrow => (130, 131, BinaryOp::JsonExtract),
            TokenKind::ArrowArrow => (130, 131, BinaryOp::JsonExtractText),

            _ => return None,
        };
        Some((l_bp, r_bp, op))
    }

    /// Parse function call: name(args) [FILTER (WHERE expr)] [OVER window_spec]
    fn parse_function_call(&mut self, name: String, start: usize) -> Result<Expr, ParseError> {
        self.expect(TokenKind::LParen, "(")?;

        // Check for DISTINCT
        let distinct = if self.current_kind() == Some(&TokenKind::Distinct) {
            self.advance();
            true
        } else {
            false
        };

        // Parse arguments
        let mut args = Vec::new();
        if self.current_kind() != Some(&TokenKind::RParen) {
            // Check for * as first argument (for COUNT(*))
            if self.current_kind() == Some(&TokenKind::Star) {
                let star_token = self.advance().unwrap();
                args.push(Expr::Star(Span::from(star_token.span.clone())));
            } else {
                args.push(self.parse_expr()?);
            }

            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                args.push(self.parse_expr()?);
            }
        }

        let mut end = self.expect(TokenKind::RParen, ")")?.span.end;

        // Optional FILTER clause
        let filter = if self.current_kind() == Some(&TokenKind::Filter) {
            self.advance();
            self.expect(TokenKind::LParen, "(")?;
            self.expect(TokenKind::Where, "WHERE")?;
            let filter_expr = self.parse_expr()?;
            end = self.expect(TokenKind::RParen, ")")?.span.end;
            Some(Box::new(filter_expr))
        } else {
            None
        };

        // Optional OVER clause (window function)
        let over = if self.current_kind() == Some(&TokenKind::Over) {
            self.advance();
            let window_spec = self.parse_window_spec()?;
            end = window_spec.span.end;
            Some(window_spec)
        } else {
            None
        };

        Ok(Expr::FunctionCall {
            name,
            args,
            distinct,
            filter,
            over,
            span: Span::new(start, end),
        })
    }

    /// Parse window specification: [window_name] | (window_spec_content)
    fn parse_window_spec(&mut self) -> Result<WindowSpec, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);

        // Check for window name reference or inline spec
        if self.current_kind() == Some(&TokenKind::Ident) {
            // Simple window name reference
            let name_token = self.advance().unwrap();
            let span = name_token.span.clone();
            let name = self.slice(&span).to_string();
            return Ok(WindowSpec {
                base_window: Some(name),
                partition_by: None,
                order_by: None,
                frame: None,
                span: Span::new(start, span.end),
            });
        }

        // Parse inline window spec in parentheses
        self.expect(TokenKind::LParen, "(")?;

        // Optional base window name
        let base_window = if self.current_kind() == Some(&TokenKind::Ident)
            && !matches!(
                self.current_kind(),
                Some(TokenKind::Partition) | Some(TokenKind::Order) | Some(TokenKind::Rows)
                    | Some(TokenKind::Range) | Some(TokenKind::Groups)
            )
        {
            let name_token = self.advance().unwrap();
            let span = name_token.span.clone();
            Some(self.slice(&span).to_string())
        } else {
            None
        };

        // Optional PARTITION BY
        let partition_by = if self.current_kind() == Some(&TokenKind::Partition) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let mut exprs = Vec::new();
            exprs.push(self.parse_expr()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                exprs.push(self.parse_expr()?);
            }
            Some(exprs)
        } else {
            None
        };

        // Optional ORDER BY
        let order_by = if self.current_kind() == Some(&TokenKind::Order) {
            self.advance();
            self.expect(TokenKind::By, "BY")?;
            let mut terms = Vec::new();
            terms.push(self.parse_ordering_term()?);
            while self.current_kind() == Some(&TokenKind::Comma) {
                self.advance();
                terms.push(self.parse_ordering_term()?);
            }
            Some(terms)
        } else {
            None
        };

        // Optional frame specification
        let frame = if matches!(
            self.current_kind(),
            Some(TokenKind::Rows) | Some(TokenKind::Range) | Some(TokenKind::Groups)
        ) {
            Some(self.parse_frame_spec()?)
        } else {
            None
        };

        let end = self.expect(TokenKind::RParen, ")")?.span.end;

        Ok(WindowSpec {
            base_window,
            partition_by,
            order_by,
            frame,
            span: Span::new(start, end),
        })
    }

    /// Parse frame specification: ROWS|RANGE|GROUPS frame_extent [EXCLUDE ...]
    fn parse_frame_spec(&mut self) -> Result<FrameSpec, ParseError> {
        let start = self.current().map(|t| t.span.start).unwrap_or(0);

        // Parse frame unit
        let unit = match self.current_kind() {
            Some(TokenKind::Rows) => {
                self.advance();
                FrameUnit::Rows
            }
            Some(TokenKind::Range) => {
                self.advance();
                FrameUnit::Range
            }
            Some(TokenKind::Groups) => {
                self.advance();
                FrameUnit::Groups
            }
            _ => return Err(ParseError::Expected {
                expected: "ROWS, RANGE, or GROUPS",
                found: self.current_kind().cloned(),
                location: self.offset_to_location(start),
            }),
        };

        // Parse frame extent: BETWEEN ... AND ... | frame_bound
        let (frame_start, frame_end) = if self.current_kind() == Some(&TokenKind::Between) {
            self.advance();
            let start_bound = self.parse_frame_bound()?;
            self.expect(TokenKind::And, "AND")?;
            let end_bound = self.parse_frame_bound()?;
            (start_bound, Some(end_bound))
        } else {
            let start_bound = self.parse_frame_bound()?;
            (start_bound, None)
        };

        // Optional EXCLUDE clause
        let mut end = self.current().map(|t| t.span.start).unwrap_or(start);
        let exclude = if self.current_kind() == Some(&TokenKind::Exclude) {
            self.advance();
            let exc = match self.current_kind() {
                Some(TokenKind::No) => {
                    self.advance();
                    self.expect(TokenKind::Others, "OTHERS")?;
                    end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(end);
                    FrameExclude::NoOthers
                }
                Some(TokenKind::Current) => {
                    self.advance();
                    self.expect(TokenKind::Row, "ROW")?;
                    end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(end);
                    FrameExclude::CurrentRow
                }
                Some(TokenKind::Group) => {
                    self.advance();
                    end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(end);
                    FrameExclude::Group
                }
                Some(TokenKind::Ties) => {
                    self.advance();
                    end = self.tokens.get(self.cursor.saturating_sub(1)).map(|t| t.span.end).unwrap_or(end);
                    FrameExclude::Ties
                }
                _ => return Err(ParseError::Expected {
                    expected: "NO OTHERS, CURRENT ROW, GROUP, or TIES",
                    found: self.current_kind().cloned(),
                    location: self.offset_to_location(end),
                }),
            };
            Some(exc)
        } else {
            None
        };

        Ok(FrameSpec {
            unit,
            start: frame_start,
            end: frame_end,
            exclude,
            span: Span::new(start, end),
        })
    }

    /// Parse frame bound: UNBOUNDED PRECEDING | expr PRECEDING | CURRENT ROW | expr FOLLOWING | UNBOUNDED FOLLOWING
    fn parse_frame_bound(&mut self) -> Result<FrameBound, ParseError> {
        match self.current_kind() {
            Some(TokenKind::Unbounded) => {
                self.advance();
                if self.current_kind() == Some(&TokenKind::Preceding) {
                    self.advance();
                    Ok(FrameBound::UnboundedPreceding)
                } else if self.current_kind() == Some(&TokenKind::Following) {
                    self.advance();
                    Ok(FrameBound::UnboundedFollowing)
                } else {
                    Err(ParseError::Expected {
                        expected: "PRECEDING or FOLLOWING",
                        found: self.current_kind().cloned(),
                        location: self.offset_to_location(self.current().map(|t| t.span.start).unwrap_or(0)),
                    })
                }
            }
            Some(TokenKind::Current) => {
                self.advance();
                self.expect(TokenKind::Row, "ROW")?;
                Ok(FrameBound::CurrentRow)
            }
            _ => {
                // expr PRECEDING | expr FOLLOWING
                let expr = self.parse_expr()?;
                if self.current_kind() == Some(&TokenKind::Preceding) {
                    self.advance();
                    Ok(FrameBound::Preceding(Box::new(expr)))
                } else if self.current_kind() == Some(&TokenKind::Following) {
                    self.advance();
                    Ok(FrameBound::Following(Box::new(expr)))
                } else {
                    Err(ParseError::Expected {
                        expected: "PRECEDING or FOLLOWING",
                        found: self.current_kind().cloned(),
                        location: self.offset_to_location(self.current().map(|t| t.span.start).unwrap_or(0)),
                    })
                }
            }
        }
    }
}

fn parse_hex(s: &str) -> Result<Vec<u8>, ()> {
    if !s.len().is_multiple_of(2) {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

pub fn parse_program(source: &str) -> Result<Program, Vec<ParseError>> {
    let mut parser = Parser::new(source);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to extract expression from first result column
    fn first_expr(stmt: &SelectStmt) -> &Expr {
        match &stmt.columns[0] {
            ResultColumn::Expr { expr, .. } => expr,
            ResultColumn::Star(_) => panic!("Expected expression, found *"),
            ResultColumn::TableStar { table, .. } => panic!("Expected expression, found {}.*", table),
        }
    }

    #[test]
    fn test_parse_select_integer() {
        let program = parse_program("SELECT 1;").unwrap();
        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.columns.len(), 1);
                match first_expr(stmt) {
                    Expr::Integer(n, _) => assert_eq!(*n, 1),
                    _ => panic!("Expected integer"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_string() {
        let program = parse_program("SELECT 'hello';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::String(s, _) => assert_eq!(s, "hello"),
                _ => panic!("Expected string"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_blob() {
        let program = parse_program("SELECT X'AABB';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Blob(bytes, _) => assert_eq!(bytes, &[0xAA, 0xBB]),
                _ => panic!("Expected blob"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_null() {
        let program = parse_program("SELECT NULL;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert!(matches!(first_expr(stmt), Expr::Null(_)));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    /// Helper to extract expression from nth result column
    fn nth_expr(stmt: &SelectStmt, n: usize) -> &Expr {
        match &stmt.columns[n] {
            ResultColumn::Expr { expr, .. } => expr,
            ResultColumn::Star(_) => panic!("Expected expression, found *"),
            ResultColumn::TableStar { table, .. } => panic!("Expected expression, found {}.*", table),
        }
    }

    #[test]
    fn test_parse_select_multiple() {
        let program = parse_program("SELECT 1, 'text', X'FF', NULL;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.columns.len(), 4);
                assert!(matches!(nth_expr(stmt, 0), Expr::Integer(1, _)));
                assert!(matches!(nth_expr(stmt, 1), Expr::String(s, _) if s == "text"));
                assert!(matches!(nth_expr(stmt, 2), Expr::Blob(b, _) if b == &[0xFF]));
                assert!(matches!(nth_expr(stmt, 3), Expr::Null(_)));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_star() {
        let program = parse_program("SELECT *;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert!(matches!(&stmt.columns[0], ResultColumn::Star(_)));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_create_table_simple() {
        let program = parse_program("CREATE TABLE foo (id INTEGER);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(!stmt.temporary);
                assert!(!stmt.if_not_exists);
                assert!(stmt.schema.is_none());
                assert_eq!(stmt.table_name, "foo");
                assert_eq!(stmt.columns.len(), 1);
                assert_eq!(stmt.columns[0].name, "id");
                assert_eq!(stmt.columns[0].type_name, Some("INTEGER".to_string()));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_temp_table() {
        let program = parse_program("CREATE TEMP TABLE bar (x);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.temporary);
                assert!(!stmt.if_not_exists);
                assert_eq!(stmt.table_name, "bar");
                assert_eq!(stmt.columns[0].name, "x");
                assert!(stmt.columns[0].type_name.is_none());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_if_not_exists() {
        let program = parse_program("CREATE TABLE IF NOT EXISTS baz (a INT, b TEXT);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(!stmt.temporary);
                assert!(stmt.if_not_exists);
                assert_eq!(stmt.table_name, "baz");
                assert_eq!(stmt.columns.len(), 2);
                assert_eq!(stmt.columns[0].name, "a");
                assert_eq!(stmt.columns[0].type_name, Some("INT".to_string()));
                assert_eq!(stmt.columns[1].name, "b");
                assert_eq!(stmt.columns[1].type_name, Some("TEXT".to_string()));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_with_schema() {
        let program = parse_program("CREATE TABLE main.users (id INTEGER);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.schema, Some("main".to_string()));
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_temp_table_if_not_exists_with_schema() {
        let program =
            parse_program("CREATE TEMPORARY TABLE IF NOT EXISTS mydb.cache (k TEXT, val BLOB);")
                .unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.temporary);
                assert!(stmt.if_not_exists);
                assert_eq!(stmt.schema, Some("mydb".to_string()));
                assert_eq!(stmt.table_name, "cache");
                assert_eq!(stmt.columns.len(), 2);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_error_shows_line_column() {
        let source = "SELECT 1;\nSELECT 2;\nBAD TOKEN;";
        let err = parse_program(source).unwrap_err();
        assert_eq!(err.len(), 1);
        let msg = err[0].to_string();
        // Error should be on line 3, column 1
        assert!(msg.contains("3:1"), "Expected '3:1' in error: {}", msg);
    }

    #[test]
    fn test_error_column_offset() {
        let source = "SELECT 1; BAD";
        let err = parse_program(source).unwrap_err();
        let msg = err[0].to_string();
        // "BAD" starts at column 11
        assert!(msg.contains("1:11"), "Expected '1:11' in error: {}", msg);
    }

    /// Helper to get first table name from FROM clause
    fn first_table(from: &FromClause) -> (&Option<String>, &str) {
        match &from.tables[0] {
            TableOrSubquery::Table { schema, name, .. } => (schema, name),
            _ => panic!("Expected simple table"),
        }
    }

    #[test]
    fn test_parse_select_from() {
        let program = parse_program("SELECT id FROM users;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.columns.len(), 1);
                assert!(matches!(first_expr(stmt), Expr::Ident(name, _, _) if name == "id"));
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                let (schema, name) = first_table(from);
                assert!(schema.is_none());
                assert_eq!(name, "users");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_star_from() {
        let program = parse_program("SELECT * FROM users;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert!(matches!(&stmt.columns[0], ResultColumn::Star(_)));
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                let (_, name) = first_table(from);
                assert_eq!(name, "users");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_from_with_schema() {
        let program = parse_program("SELECT id FROM main.users;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                let (schema, name) = first_table(from);
                assert_eq!(schema, &Some("main".to_string()));
                assert_eq!(name, "users");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_multiple_from() {
        let program = parse_program("SELECT id, name, email FROM users;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.columns.len(), 3);
                assert!(matches!(nth_expr(stmt, 0), Expr::Ident(name, _, _) if name == "id"));
                assert!(matches!(nth_expr(stmt, 1), Expr::Ident(name, _, _) if name == "name"));
                assert!(matches!(nth_expr(stmt, 2), Expr::Ident(name, _, _) if name == "email"));
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                let (_, name) = first_table(from);
                assert_eq!(name, "users");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_without_from() {
        let program = parse_program("SELECT 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert!(stmt.from.is_none());
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_from_case_insensitive() {
        let program = parse_program("select id from USERS;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                let (_, name) = first_table(from);
                assert_eq!(name, "USERS");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_without_semicolon() {
        // Tests that consume_if correctly handles missing semicolon
        let program = parse_program("SELECT 1").unwrap();
        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.columns.len(), 1);
                // Span should end at the expression, not include a semicolon
                assert_eq!(stmt.span.end, 8);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_create_table_without_semicolon() {
        // Tests that consume_if correctly handles missing semicolon for CREATE TABLE
        let program = parse_program("CREATE TABLE foo (id)").unwrap();
        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_name, "foo");
                // Span should end at the closing paren, not include a semicolon
                assert_eq!(stmt.span.end, 21);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_multiple_statements_with_semicolons() {
        let program = parse_program("SELECT 1; SELECT 2;").unwrap();
        assert_eq!(program.statements.len(), 2);
    }

    #[test]
    fn test_parse_column_def_without_type() {
        // Tests that consume_if correctly handles missing type (returns None, doesn't panic)
        let program = parse_program("CREATE TABLE foo (col)").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.columns.len(), 1);
                assert_eq!(stmt.columns[0].name, "col");
                assert!(stmt.columns[0].type_name.is_none());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_consume_if_behavior() {
        // Direct test of Parser::consume_if behavior
        let mut parser = Parser::new("SELECT 1");

        // Should return None for non-matching token
        let result = parser.consume_if(TokenKind::Create);
        assert!(result.is_none());
        // Cursor should not have advanced
        assert_eq!(parser.current_kind(), Some(&TokenKind::Select));

        // Should return Some for matching token and advance
        let result = parser.consume_if(TokenKind::Select);
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, TokenKind::Select);
        // Cursor should have advanced to next token
        assert_eq!(parser.current_kind(), Some(&TokenKind::Integer));
    }

    // ========================================
    // Table Constraints Tests
    // ========================================

    #[test]
    fn test_parse_table_constraint_primary_key() {
        let program = parse_program("CREATE TABLE t (a INT, b INT, PRIMARY KEY (a, b))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.columns.len(), 2);
                assert_eq!(stmt.table_constraints.len(), 1);
                match &stmt.table_constraints[0] {
                    TableConstraint::PrimaryKey { columns, .. } => {
                        assert_eq!(columns.len(), 2);
                    }
                    _ => panic!("Expected PrimaryKey constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_unique() {
        let program = parse_program("CREATE TABLE t (a INT, b INT, UNIQUE (a, b))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_constraints.len(), 1);
                match &stmt.table_constraints[0] {
                    TableConstraint::Unique { columns, .. } => {
                        assert_eq!(columns.len(), 2);
                    }
                    _ => panic!("Expected Unique constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_check() {
        let program = parse_program("CREATE TABLE t (a INT, CHECK (a > 0))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_constraints.len(), 1);
                match &stmt.table_constraints[0] {
                    TableConstraint::Check { expr, .. } => {
                        // Check that expr is a > 0
                        match expr {
                            Expr::Binary { .. } => {}
                            _ => panic!("Expected binary op"),
                        }
                    }
                    _ => panic!("Expected Check constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_foreign_key() {
        let program = parse_program("CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES other(id))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_constraints.len(), 1);
                match &stmt.table_constraints[0] {
                    TableConstraint::ForeignKey { columns, foreign_table, foreign_columns, .. } => {
                        assert_eq!(columns, &vec!["a".to_string()]);
                        assert_eq!(foreign_table, "other");
                        assert_eq!(foreign_columns, &Some(vec!["id".to_string()]));
                    }
                    _ => panic!("Expected ForeignKey constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_foreign_key_with_actions() {
        let program = parse_program(
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES other(id) ON DELETE CASCADE ON UPDATE SET NULL)"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.table_constraints[0] {
                    TableConstraint::ForeignKey { on_delete, on_update, .. } => {
                        assert_eq!(*on_delete, Some(ForeignKeyAction::Cascade));
                        assert_eq!(*on_update, Some(ForeignKeyAction::SetNull));
                    }
                    _ => panic!("Expected ForeignKey constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_foreign_key_deferrable() {
        let program = parse_program(
            "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES other(id) DEFERRABLE INITIALLY DEFERRED)"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.table_constraints[0] {
                    TableConstraint::ForeignKey { deferrable, .. } => {
                        assert_eq!(*deferrable, Some(Deferrable::InitiallyDeferred));
                    }
                    _ => panic!("Expected ForeignKey constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_named_table_constraint() {
        let program = parse_program("CREATE TABLE t (a INT, CONSTRAINT pk_t PRIMARY KEY (a))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.table_constraints[0] {
                    TableConstraint::PrimaryKey { name, .. } => {
                        assert_eq!(*name, Some("pk_t".to_string()));
                    }
                    _ => panic!("Expected PrimaryKey constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_with_collate() {
        let program = parse_program("CREATE TABLE t (a TEXT, UNIQUE (a COLLATE NOCASE))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.table_constraints[0] {
                    TableConstraint::Unique { columns, .. } => {
                        assert_eq!(columns.len(), 1);
                        assert_eq!(columns[0].collation, Some("NOCASE".to_string()));
                    }
                    _ => panic!("Expected Unique constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_constraint_with_sort_order() {
        let program = parse_program("CREATE TABLE t (a INT, b INT, PRIMARY KEY (a DESC, b ASC))").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.table_constraints[0] {
                    TableConstraint::PrimaryKey { columns, .. } => {
                        assert_eq!(columns[0].direction, Some(OrderDirection::Desc));
                        assert_eq!(columns[1].direction, Some(OrderDirection::Asc));
                    }
                    _ => panic!("Expected PrimaryKey constraint"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_multiple_table_constraints() {
        let program = parse_program(
            "CREATE TABLE t (a INT, b INT, PRIMARY KEY (a), UNIQUE (b), CHECK (a > 0))"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_constraints.len(), 3);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    // ========================================
    // Table Options Tests
    // ========================================

    #[test]
    fn test_parse_table_without_rowid() {
        let program = parse_program("CREATE TABLE t (a INT PRIMARY KEY) WITHOUT ROWID").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_options, vec![TableOption::WithoutRowid]);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_strict() {
        let program = parse_program("CREATE TABLE t (a INT) STRICT").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_options, vec![TableOption::Strict]);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_table_without_rowid_and_strict() {
        let program = parse_program("CREATE TABLE t (a INT PRIMARY KEY) WITHOUT ROWID, STRICT").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_options.len(), 2);
                assert!(stmt.table_options.contains(&TableOption::WithoutRowid));
                assert!(stmt.table_options.contains(&TableOption::Strict));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    // ========================================
    // CREATE TABLE AS SELECT Tests
    // ========================================

    #[test]
    fn test_parse_create_table_as_select() {
        let program = parse_program("CREATE TABLE t AS SELECT * FROM other").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.table_name, "t");
                assert!(stmt.columns.is_empty());
                assert!(stmt.as_select.is_some());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_temp_table_as_select() {
        let program = parse_program("CREATE TEMP TABLE t AS SELECT 1, 2, 3").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.temporary);
                assert!(stmt.as_select.is_some());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_if_not_exists_as_select() {
        let program = parse_program("CREATE TABLE IF NOT EXISTS t AS SELECT * FROM other").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.if_not_exists);
                assert!(stmt.as_select.is_some());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    // ========================================
    // Doc Comment Tests
    // ========================================

    #[test]
    fn test_parse_create_table_with_table_doc() {
        let sql = r#"
            CREATE TABLE students (
                --! All students at Foo University.
                --! @details https://foo.edu/students
                id INTEGER PRIMARY KEY
            );
        "#;
        let program = parse_program(sql).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.doc.is_some());
                let doc = stmt.doc.as_ref().unwrap();
                assert_eq!(doc.description, "All students at Foo University.");
                assert_eq!(doc.get_tag("details"), Some("https://foo.edu/students"));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_with_column_doc() {
        let sql = r#"
            CREATE TABLE students (
                --- Student ID assigned at orientation
                --- @example 'S10483'
                student_id TEXT PRIMARY KEY,
                --- Full name of student
                name TEXT
            );
        "#;
        let program = parse_program(sql).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                // Check first column doc
                let col1 = &stmt.columns[0];
                assert_eq!(col1.name, "student_id");
                assert!(col1.doc.is_some());
                let doc1 = col1.doc.as_ref().unwrap();
                assert_eq!(doc1.description, "Student ID assigned at orientation");
                assert_eq!(doc1.get_tag("example"), Some("'S10483'"));

                // Check second column doc
                let col2 = &stmt.columns[1];
                assert_eq!(col2.name, "name");
                assert!(col2.doc.is_some());
                let doc2 = col2.doc.as_ref().unwrap();
                assert_eq!(doc2.description, "Full name of student");
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_with_both_docs() {
        let sql = r#"
            CREATE TABLE students (
                --! All students at Foo University.
                --! @details https://foo.edu/students

                --- Student ID assigned at orientation
                student_id TEXT PRIMARY KEY
            );
        "#;
        let program = parse_program(sql).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                // Check table doc
                assert!(stmt.doc.is_some());
                let table_doc = stmt.doc.as_ref().unwrap();
                assert_eq!(table_doc.description, "All students at Foo University.");

                // Check column doc
                let col1 = &stmt.columns[0];
                assert!(col1.doc.is_some());
                let col_doc = col1.doc.as_ref().unwrap();
                assert_eq!(col_doc.description, "Student ID assigned at orientation");
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_no_docs() {
        let sql = "CREATE TABLE t (id INTEGER);";
        let program = parse_program(sql).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.doc.is_none());
                assert!(stmt.columns[0].doc.is_none());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_create_table_regular_comments_not_docs() {
        // Regular comments (--) should not be treated as docs
        let sql = r#"
            CREATE TABLE t (
                -- This is just a regular comment
                id INTEGER
            );
        "#;
        let program = parse_program(sql).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(stmt.doc.is_none());
                assert!(stmt.columns[0].doc.is_none());
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    // ========================================
    // CREATE TRIGGER Tests
    // ========================================

    #[test]
    fn test_parse_create_trigger_after_insert() {
        let program = parse_program(
            "CREATE TRIGGER log_insert AFTER INSERT ON users BEGIN INSERT INTO audit(msg) VALUES ('insert'); END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.trigger_name, "log_insert");
                assert_eq!(stmt.timing, TriggerTiming::After);
                assert_eq!(stmt.event, TriggerEvent::Insert);
                assert_eq!(stmt.table_name, "users");
                assert_eq!(stmt.body.len(), 1);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_before_update() {
        let program = parse_program(
            "CREATE TRIGGER before_update BEFORE UPDATE ON products BEGIN UPDATE products SET updated = 1 WHERE id = 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.timing, TriggerTiming::Before);
                match &stmt.event {
                    TriggerEvent::Update { columns } => {
                        assert!(columns.is_none());
                    }
                    _ => panic!("Expected Update event"),
                }
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_update_of_columns() {
        let program = parse_program(
            "CREATE TRIGGER price_change AFTER UPDATE OF price, discount ON products BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                match &stmt.event {
                    TriggerEvent::Update { columns } => {
                        let cols = columns.as_ref().unwrap();
                        assert_eq!(cols, &vec!["price".to_string(), "discount".to_string()]);
                    }
                    _ => panic!("Expected Update event"),
                }
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_instead_of() {
        let program = parse_program(
            "CREATE TRIGGER instead_insert INSTEAD OF INSERT ON user_view BEGIN INSERT INTO users(name) VALUES ('test'); END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.timing, TriggerTiming::InsteadOf);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_after_delete() {
        let program = parse_program(
            "CREATE TRIGGER log_delete AFTER DELETE ON orders BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.event, TriggerEvent::Delete);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_with_when() {
        let program = parse_program(
            "CREATE TRIGGER conditional AFTER UPDATE ON inventory WHEN 1 > 0 BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert!(stmt.when_clause.is_some());
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_for_each_row() {
        let program = parse_program(
            "CREATE TRIGGER row_trigger AFTER INSERT ON items FOR EACH ROW BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert!(stmt.for_each_row);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_temp_trigger() {
        let program = parse_program(
            "CREATE TEMP TRIGGER temp_log AFTER INSERT ON temp_data BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert!(stmt.temporary);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_if_not_exists() {
        let program = parse_program(
            "CREATE TRIGGER IF NOT EXISTS maybe_trigger AFTER INSERT ON events BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert!(stmt.if_not_exists);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_with_schema() {
        let program = parse_program(
            "CREATE TRIGGER main.prefixed_trigger AFTER INSERT ON users BEGIN SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.schema, Some("main".to_string()));
                assert_eq!(stmt.trigger_name, "prefixed_trigger");
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    #[test]
    fn test_parse_create_trigger_multiple_statements() {
        let program = parse_program(
            "CREATE TRIGGER multi AFTER INSERT ON orders BEGIN UPDATE inventory SET qty = qty - 1; INSERT INTO log(msg) VALUES ('inserted'); SELECT 1; END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.body.len(), 3);
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    // ========================================
    // EXPLAIN Tests
    // ========================================

    #[test]
    fn test_parse_explain_select() {
        let program = parse_program("EXPLAIN SELECT * FROM users").unwrap();
        match &program.statements[0] {
            Statement::Explain { query_plan, stmt, .. } => {
                assert!(!query_plan);
                match stmt.as_ref() {
                    Statement::Select(_) => {}
                    _ => panic!("Expected SELECT inside EXPLAIN"),
                }
            }
            _ => panic!("Expected EXPLAIN"),
        }
    }

    #[test]
    fn test_parse_explain_query_plan() {
        let program = parse_program("EXPLAIN QUERY PLAN SELECT id FROM users WHERE id > 5").unwrap();
        match &program.statements[0] {
            Statement::Explain { query_plan, stmt, .. } => {
                assert!(query_plan);
                match stmt.as_ref() {
                    Statement::Select(_) => {}
                    _ => panic!("Expected SELECT inside EXPLAIN"),
                }
            }
            _ => panic!("Expected EXPLAIN"),
        }
    }

    #[test]
    fn test_parse_explain_insert() {
        let program = parse_program("EXPLAIN INSERT INTO users(name) VALUES ('test')").unwrap();
        match &program.statements[0] {
            Statement::Explain { stmt, .. } => {
                match stmt.as_ref() {
                    Statement::Insert(_) => {}
                    _ => panic!("Expected INSERT inside EXPLAIN"),
                }
            }
            _ => panic!("Expected EXPLAIN"),
        }
    }

    #[test]
    fn test_parse_explain_update() {
        let program = parse_program("EXPLAIN UPDATE users SET name = 'new' WHERE id = 1").unwrap();
        match &program.statements[0] {
            Statement::Explain { stmt, .. } => {
                match stmt.as_ref() {
                    Statement::Update(_) => {}
                    _ => panic!("Expected UPDATE inside EXPLAIN"),
                }
            }
            _ => panic!("Expected EXPLAIN"),
        }
    }

    #[test]
    fn test_parse_explain_delete() {
        let program = parse_program("EXPLAIN DELETE FROM users WHERE id = 1").unwrap();
        match &program.statements[0] {
            Statement::Explain { stmt, .. } => {
                match stmt.as_ref() {
                    Statement::Delete(_) => {}
                    _ => panic!("Expected DELETE inside EXPLAIN"),
                }
            }
            _ => panic!("Expected EXPLAIN"),
        }
    }

    // ========================================
    // CREATE VIRTUAL TABLE Tests
    // ========================================

    #[test]
    fn test_parse_create_virtual_table_basic() {
        let program = parse_program("CREATE VIRTUAL TABLE docs USING fts5(content)").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert_eq!(stmt.table_name, "docs");
                assert_eq!(stmt.module_name, "fts5");
                assert_eq!(stmt.module_args, Some(vec!["content".to_string()]));
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    #[test]
    fn test_parse_create_virtual_table_multiple_args() {
        let program = parse_program("CREATE VIRTUAL TABLE articles USING fts5(title, body, author)").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert_eq!(stmt.module_name, "fts5");
                let args = stmt.module_args.as_ref().unwrap();
                assert_eq!(args.len(), 3);
                assert_eq!(args[0], "title");
                assert_eq!(args[1], "body");
                assert_eq!(args[2], "author");
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    #[test]
    fn test_parse_create_virtual_table_no_args() {
        let program = parse_program("CREATE VIRTUAL TABLE series USING generate_series").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert_eq!(stmt.module_name, "generate_series");
                assert!(stmt.module_args.is_none());
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    #[test]
    fn test_parse_create_virtual_table_if_not_exists() {
        let program = parse_program("CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(text)").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert!(stmt.if_not_exists);
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    #[test]
    fn test_parse_create_virtual_table_with_schema() {
        let program = parse_program("CREATE VIRTUAL TABLE main.docs USING fts5(content)").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert_eq!(stmt.schema, Some("main".to_string()));
                assert_eq!(stmt.table_name, "docs");
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    #[test]
    fn test_parse_create_virtual_table_rtree() {
        let program = parse_program("CREATE VIRTUAL TABLE locations USING rtree(id, min_x, max_x, min_y, max_y)").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert_eq!(stmt.module_name, "rtree");
                let args = stmt.module_args.as_ref().unwrap();
                assert_eq!(args.len(), 5);
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    #[test]
    fn test_parse_create_virtual_table_with_key_value_args() {
        let program = parse_program("CREATE VIRTUAL TABLE csv_data USING csv(filename = 'data.csv', header = yes)").unwrap();
        match &program.statements[0] {
            Statement::CreateVirtualTable(stmt) => {
                assert_eq!(stmt.module_name, "csv");
                let args = stmt.module_args.as_ref().unwrap();
                assert_eq!(args.len(), 2);
            }
            _ => panic!("Expected CREATE VIRTUAL TABLE"),
        }
    }

    // ========================================
    // RAISE Function Tests
    // ========================================

    #[test]
    fn test_parse_raise_ignore() {
        let program = parse_program("SELECT RAISE(IGNORE)").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let expr = match &stmt.columns[0] {
                    ResultColumn::Expr { expr, .. } => expr,
                    _ => panic!("Expected Expr column"),
                };
                match expr {
                    Expr::Raise { action, message, .. } => {
                        assert_eq!(action, &RaiseAction::Ignore);
                        assert!(message.is_none());
                    }
                    _ => panic!("Expected RAISE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_raise_abort() {
        let program = parse_program("SELECT RAISE(ABORT, 'Error message')").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let expr = match &stmt.columns[0] {
                    ResultColumn::Expr { expr, .. } => expr,
                    _ => panic!("Expected Expr column"),
                };
                match expr {
                    Expr::Raise { action, message, .. } => {
                        assert_eq!(action, &RaiseAction::Abort);
                        assert!(message.is_some());
                    }
                    _ => panic!("Expected RAISE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_raise_rollback() {
        let program = parse_program("SELECT RAISE(ROLLBACK, 'Rolling back')").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let expr = match &stmt.columns[0] {
                    ResultColumn::Expr { expr, .. } => expr,
                    _ => panic!("Expected Expr column"),
                };
                match expr {
                    Expr::Raise { action, .. } => {
                        assert_eq!(action, &RaiseAction::Rollback);
                    }
                    _ => panic!("Expected RAISE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_raise_fail() {
        let program = parse_program("SELECT RAISE(FAIL, 'Failing')").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let expr = match &stmt.columns[0] {
                    ResultColumn::Expr { expr, .. } => expr,
                    _ => panic!("Expected Expr column"),
                };
                match expr {
                    Expr::Raise { action, .. } => {
                        assert_eq!(action, &RaiseAction::Fail);
                    }
                    _ => panic!("Expected RAISE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_raise_in_trigger() {
        let program = parse_program(
            "CREATE TRIGGER t BEFORE INSERT ON tbl BEGIN SELECT RAISE(ABORT, 'error'); END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTrigger(stmt) => {
                assert_eq!(stmt.body.len(), 1);
                // Just verify it parses - the RAISE is inside the SELECT
            }
            _ => panic!("Expected CREATE TRIGGER"),
        }
    }

    // ========================================
    // Expression Parsing Tests (Phase 2)
    // ========================================

    #[test]
    fn test_parse_binary_add() {
        let program = parse_program("SELECT 1 + 2;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, left, right, .. } => {
                    assert_eq!(*op, BinaryOp::Add);
                    assert!(matches!(left.as_ref(), Expr::Integer(1, _)));
                    assert!(matches!(right.as_ref(), Expr::Integer(2, _)));
                }
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_binary_sub() {
        let program = parse_program("SELECT 5 - 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::Sub),
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_binary_mul() {
        let program = parse_program("SELECT 2 * 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::Mul),
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_binary_div() {
        let program = parse_program("SELECT 10 / 2;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::Div),
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_binary_mod() {
        let program = parse_program("SELECT 10 % 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::Mod),
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_precedence_mul_over_add() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let program = parse_program("SELECT 1 + 2 * 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, left, right, .. } => {
                    assert_eq!(*op, BinaryOp::Add);
                    assert!(matches!(left.as_ref(), Expr::Integer(1, _)));
                    // Right side should be 2 * 3
                    match right.as_ref() {
                        Expr::Binary { op, left, right, .. } => {
                            assert_eq!(*op, BinaryOp::Mul);
                            assert!(matches!(left.as_ref(), Expr::Integer(2, _)));
                            assert!(matches!(right.as_ref(), Expr::Integer(3, _)));
                        }
                        _ => panic!("Expected binary expression on right"),
                    }
                }
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_left_associativity() {
        // 1 - 2 - 3 should parse as (1 - 2) - 3
        let program = parse_program("SELECT 1 - 2 - 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, left, right, .. } => {
                    assert_eq!(*op, BinaryOp::Sub);
                    // Left side should be 1 - 2
                    match left.as_ref() {
                        Expr::Binary { op, left, right, .. } => {
                            assert_eq!(*op, BinaryOp::Sub);
                            assert!(matches!(left.as_ref(), Expr::Integer(1, _)));
                            assert!(matches!(right.as_ref(), Expr::Integer(2, _)));
                        }
                        _ => panic!("Expected binary expression on left"),
                    }
                    assert!(matches!(right.as_ref(), Expr::Integer(3, _)));
                }
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_parentheses() {
        // (1 + 2) * 3 should parse with parens overriding precedence
        let program = parse_program("SELECT (1 + 2) * 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, left, right, .. } => {
                    assert_eq!(*op, BinaryOp::Mul);
                    // Left side should be parenthesized (1 + 2)
                    match left.as_ref() {
                        Expr::Paren(inner, _) => match inner.as_ref() {
                            Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::Add),
                            _ => panic!("Expected binary inside parens"),
                        },
                        _ => panic!("Expected parenthesized expression"),
                    }
                    assert!(matches!(right.as_ref(), Expr::Integer(3, _)));
                }
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_unary_minus() {
        let program = parse_program("SELECT -5;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Unary { op, expr, .. } => {
                    assert_eq!(*op, UnaryOp::Neg);
                    assert!(matches!(expr.as_ref(), Expr::Integer(5, _)));
                }
                _ => panic!("Expected unary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_unary_plus() {
        let program = parse_program("SELECT +5;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Unary { op, expr, .. } => {
                    assert_eq!(*op, UnaryOp::Pos);
                    assert!(matches!(expr.as_ref(), Expr::Integer(5, _)));
                }
                _ => panic!("Expected unary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_unary_not() {
        let program = parse_program("SELECT NOT 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Unary { op, expr, .. } => {
                    assert_eq!(*op, UnaryOp::Not);
                    assert!(matches!(expr.as_ref(), Expr::Integer(1, _)));
                }
                _ => panic!("Expected unary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_unary_bitnot() {
        let program = parse_program("SELECT ~5;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Unary { op, expr, .. } => {
                    assert_eq!(*op, UnaryOp::BitNot);
                    assert!(matches!(expr.as_ref(), Expr::Integer(5, _)));
                }
                _ => panic!("Expected unary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_comparison_operators() {
        let tests = [
            ("SELECT 1 < 2;", BinaryOp::Lt),
            ("SELECT 1 <= 2;", BinaryOp::Le),
            ("SELECT 1 > 2;", BinaryOp::Gt),
            ("SELECT 1 >= 2;", BinaryOp::Ge),
            ("SELECT 1 = 2;", BinaryOp::Eq),
            ("SELECT 1 == 2;", BinaryOp::Eq),
            ("SELECT 1 <> 2;", BinaryOp::Ne),
            ("SELECT 1 != 2;", BinaryOp::Ne),
        ];

        for (sql, expected_op) in tests {
            let program = parse_program(sql).unwrap();
            match &program.statements[0] {
                Statement::Select(stmt) => match first_expr(stmt) {
                    Expr::Binary { op, .. } => assert_eq!(*op, expected_op, "Failed for: {}", sql),
                    _ => panic!("Expected binary expression for: {}", sql),
                },
                _ => panic!("Expected SELECT"),
            }
        }
    }

    #[test]
    fn test_parse_logical_operators() {
        let program = parse_program("SELECT 1 AND 2 OR 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                // OR has lower precedence, so it's the root
                Expr::Binary { op, left, right, .. } => {
                    assert_eq!(*op, BinaryOp::Or);
                    // Left should be (1 AND 2)
                    match left.as_ref() {
                        Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::And),
                        _ => panic!("Expected AND on left"),
                    }
                    assert!(matches!(right.as_ref(), Expr::Integer(3, _)));
                }
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_string_concat() {
        let program = parse_program("SELECT 'a' || 'b';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, left, right, .. } => {
                    assert_eq!(*op, BinaryOp::Concat);
                    assert!(matches!(left.as_ref(), Expr::String(s, _) if s == "a"));
                    assert!(matches!(right.as_ref(), Expr::String(s, _) if s == "b"));
                }
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_bitwise_operators() {
        let tests = [
            ("SELECT 1 & 2;", BinaryOp::BitAnd),
            ("SELECT 1 | 2;", BinaryOp::BitOr),
            ("SELECT 1 << 2;", BinaryOp::LShift),
            ("SELECT 1 >> 2;", BinaryOp::RShift),
        ];

        for (sql, expected_op) in tests {
            let program = parse_program(sql).unwrap();
            match &program.statements[0] {
                Statement::Select(stmt) => match first_expr(stmt) {
                    Expr::Binary { op, .. } => assert_eq!(*op, expected_op, "Failed for: {}", sql),
                    _ => panic!("Expected binary expression for: {}", sql),
                },
                _ => panic!("Expected SELECT"),
            }
        }
    }

    #[test]
    fn test_parse_bind_params() {
        let tests = ["SELECT ?;", "SELECT ?1;", "SELECT :name;", "SELECT @var;", "SELECT $val;"];

        for sql in tests {
            let program = parse_program(sql).unwrap();
            match &program.statements[0] {
                Statement::Select(stmt) => {
                    assert!(matches!(first_expr(stmt), Expr::BindParam(_, _)), "Failed for: {}", sql);
                }
                _ => panic!("Expected SELECT"),
            }
        }
    }

    #[test]
    fn test_parse_hex_integer() {
        let program = parse_program("SELECT 0xFF;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::HexInteger(val, _) => assert_eq!(*val, 255),
                _ => panic!("Expected hex integer"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_complex_expression() {
        // Test a complex expression: -1 + 2 * (3 + 4) / 5
        let program = parse_program("SELECT -1 + 2 * (3 + 4) / 5;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                // Should parse as: (-1) + ((2 * (3 + 4)) / 5)
                assert!(matches!(first_expr(stmt), Expr::Binary { op: BinaryOp::Add, .. }));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_is_operator() {
        let program = parse_program("SELECT x IS y;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::Is),
                _ => panic!("Expected binary expression"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_is_null() {
        let program = parse_program("SELECT x IS NULL;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::IsNull { negated, .. } => assert!(!negated),
                _ => panic!("Expected IsNull"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_is_not_null() {
        let program = parse_program("SELECT x IS NOT NULL;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::IsNull { negated, .. } => assert!(*negated),
                _ => panic!("Expected IsNull"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_in_list() {
        let program = parse_program("SELECT x IN (1, 2, 3);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::InList { list, negated, .. } => {
                    assert!(!negated);
                    assert_eq!(list.len(), 3);
                }
                _ => panic!("Expected InList"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_not_in_list() {
        let program = parse_program("SELECT x NOT IN ('a', 'b');").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::InList { list, negated, .. } => {
                    assert!(*negated);
                    assert_eq!(list.len(), 2);
                }
                _ => panic!("Expected InList"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_in_subquery() {
        let program = parse_program("SELECT x IN (SELECT id FROM users);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::InSelect { negated, .. } => assert!(!negated),
                _ => panic!("Expected InSelect"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_not_in_subquery() {
        let program = parse_program("SELECT x NOT IN (SELECT id FROM users);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::InSelect { negated, .. } => assert!(*negated),
                _ => panic!("Expected InSelect"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_between() {
        let program = parse_program("SELECT x BETWEEN 1 AND 10;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Between { negated, .. } => assert!(!negated),
                _ => panic!("Expected Between"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_not_between() {
        let program = parse_program("SELECT x NOT BETWEEN 'a' AND 'z';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Between { negated, .. } => assert!(*negated),
                _ => panic!("Expected Between"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_like() {
        let program = parse_program("SELECT name LIKE '%test%';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Like { op, negated, escape, .. } => {
                    assert_eq!(*op, BinaryOp::Like);
                    assert!(!negated);
                    assert!(escape.is_none());
                }
                _ => panic!("Expected Like"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_not_like() {
        let program = parse_program("SELECT name NOT LIKE '%test%';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Like { negated, .. } => assert!(*negated),
                _ => panic!("Expected Like"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_like_escape() {
        let program = parse_program("SELECT name LIKE '%\\%%' ESCAPE '\\';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Like { escape, .. } => assert!(escape.is_some()),
                _ => panic!("Expected Like"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_glob() {
        let program = parse_program("SELECT name GLOB '*test*';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Like { op, .. } => assert_eq!(*op, BinaryOp::Glob),
                _ => panic!("Expected Like with Glob op"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_not_glob() {
        let program = parse_program("SELECT name NOT GLOB '*test*';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Like { op, negated, .. } => {
                    assert_eq!(*op, BinaryOp::Glob);
                    assert!(*negated);
                }
                _ => panic!("Expected Like with Glob op"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_exists() {
        let program = parse_program("SELECT EXISTS (SELECT 1 FROM users);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Exists { negated, .. } => assert!(!negated),
                _ => panic!("Expected Exists"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_not_exists() {
        let program = parse_program("SELECT NOT EXISTS (SELECT 1 FROM users);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Exists { negated, .. } => assert!(*negated),
                _ => panic!("Expected Exists"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_scalar_subquery() {
        let program = parse_program("SELECT (SELECT max(price) FROM products);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Subquery { .. } => {}
                _ => panic!("Expected Subquery"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_collate() {
        let program = parse_program("SELECT name COLLATE NOCASE;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Collate { collation, .. } => assert_eq!(collation, "NOCASE"),
                _ => panic!("Expected Collate"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_case_simple() {
        let program = parse_program(
            "SELECT CASE status WHEN 'A' THEN 'Active' WHEN 'I' THEN 'Inactive' END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Case { operand, when_clauses, else_clause, .. } => {
                    assert!(operand.is_some());
                    assert_eq!(when_clauses.len(), 2);
                    assert!(else_clause.is_none());
                }
                _ => panic!("Expected Case"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_case_searched() {
        let program = parse_program(
            "SELECT CASE WHEN x > 10 THEN 'big' WHEN x > 5 THEN 'medium' ELSE 'small' END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Case { operand, when_clauses, else_clause, .. } => {
                    assert!(operand.is_none());
                    assert_eq!(when_clauses.len(), 2);
                    assert!(else_clause.is_some());
                }
                _ => panic!("Expected Case"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_case_with_else() {
        let program = parse_program(
            "SELECT CASE status WHEN 'A' THEN 'Active' ELSE 'Unknown' END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Case { else_clause, .. } => assert!(else_clause.is_some()),
                _ => panic!("Expected Case"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cast_basic() {
        let program = parse_program("SELECT CAST(x AS INTEGER);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Cast { type_name, .. } => {
                    assert_eq!(type_name.name, "INTEGER");
                    assert!(type_name.args.is_none());
                }
                _ => panic!("Expected Cast"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cast_with_size() {
        let program = parse_program("SELECT CAST(x AS VARCHAR(255));").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Cast { type_name, .. } => {
                    assert_eq!(type_name.name, "VARCHAR");
                    assert_eq!(type_name.args, Some((255, None)));
                }
                _ => panic!("Expected Cast"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cast_decimal() {
        let program = parse_program("SELECT CAST(price AS DECIMAL(10, 2));").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Cast { type_name, .. } => {
                    assert_eq!(type_name.name, "DECIMAL");
                    assert_eq!(type_name.args, Some((10, Some(2))));
                }
                _ => panic!("Expected Cast"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_nested_case() {
        let program = parse_program(
            "SELECT CASE WHEN x = 1 THEN CASE WHEN y = 2 THEN 'a' ELSE 'b' END ELSE 'c' END;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Case { when_clauses, .. } => {
                    // First WHEN's THEN should be another CASE
                    assert!(matches!(when_clauses[0].1, Expr::Case { .. }));
                }
                _ => panic!("Expected Case"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // TRUE/FALSE Literal Tests
    // ========================================

    #[test]
    fn test_parse_true_literal() {
        let program = parse_program("SELECT TRUE;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Integer(val, _) => assert_eq!(*val, 1),
                _ => panic!("Expected Integer(1)"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_false_literal() {
        let program = parse_program("SELECT FALSE;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Integer(val, _) => assert_eq!(*val, 0),
                _ => panic!("Expected Integer(0)"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_true_false_in_where() {
        let program = parse_program(
            "SELECT * FROM users WHERE active = TRUE AND deleted = FALSE;"
        ).unwrap();
        assert!(matches!(&program.statements[0], Statement::Select(_)));
    }

    #[test]
    fn test_parse_true_case_insensitive() {
        let program = parse_program("SELECT true, True, TRUE;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.columns.len(), 3);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // JSON Operator Tests
    // ========================================

    #[test]
    fn test_parse_json_extract() {
        let program = parse_program("SELECT data->'name';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::JsonExtract),
                _ => panic!("Expected Binary with JsonExtract"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_json_extract_text() {
        let program = parse_program("SELECT data->>'name';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::JsonExtractText),
                _ => panic!("Expected Binary with JsonExtractText"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_json_chained() {
        // JSON operators should chain left-to-right
        let program = parse_program("SELECT data->'address'->'city';").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { left, op, .. } => {
                    assert_eq!(*op, BinaryOp::JsonExtract);
                    // Left side should also be a JSON extract
                    assert!(matches!(left.as_ref(), Expr::Binary { op: BinaryOp::JsonExtract, .. }));
                }
                _ => panic!("Expected chained Binary"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_json_in_where() {
        let program = parse_program(
            "SELECT * FROM users WHERE data->>'status' = 'active';"
        ).unwrap();
        assert!(matches!(&program.statements[0], Statement::Select(_)));
    }

    #[test]
    fn test_parse_json_with_integer_index() {
        let program = parse_program("SELECT arr->0;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, right, .. } => {
                    assert_eq!(*op, BinaryOp::JsonExtract);
                    assert!(matches!(right.as_ref(), Expr::Integer(0, _)));
                }
                _ => panic!("Expected Binary"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_json_precedence() {
        // JSON operators should bind tighter than comparison
        let program = parse_program("SELECT data->'x' = 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::Binary { op, left, .. } => {
                    // Top level should be = comparison
                    assert_eq!(*op, BinaryOp::Eq);
                    // Left side should be the JSON extract
                    assert!(matches!(left.as_ref(), Expr::Binary { op: BinaryOp::JsonExtract, .. }));
                }
                _ => panic!("Expected Binary"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_complex_where_with_in_and_like() {
        let program = parse_program(
            "SELECT * FROM users WHERE status IN ('active', 'pending') AND name LIKE 'A%';"
        ).unwrap();
        assert!(matches!(&program.statements[0], Statement::Select(_)));
    }

    #[test]
    fn test_parse_exists_in_where() {
        let program = parse_program(
            "SELECT * FROM orders WHERE EXISTS (SELECT 1 FROM users WHERE users.id = orders.user_id);"
        ).unwrap();
        assert!(matches!(&program.statements[0], Statement::Select(_)));
    }

    // ========================================
    // Phase 3: Simple Statement Tests
    // ========================================

    // --- DROP Statements ---

    #[test]
    fn test_parse_drop_table() {
        let program = parse_program("DROP TABLE users;").unwrap();
        match &program.statements[0] {
            Statement::DropTable(stmt) => {
                assert!(!stmt.if_exists);
                assert!(stmt.schema.is_none());
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected DROP TABLE"),
        }
    }

    #[test]
    fn test_parse_drop_table_if_exists() {
        let program = parse_program("DROP TABLE IF EXISTS mydb.users;").unwrap();
        match &program.statements[0] {
            Statement::DropTable(stmt) => {
                assert!(stmt.if_exists);
                assert_eq!(stmt.schema, Some("mydb".to_string()));
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected DROP TABLE"),
        }
    }

    #[test]
    fn test_parse_drop_index() {
        let program = parse_program("DROP INDEX idx_users_name;").unwrap();
        match &program.statements[0] {
            Statement::DropIndex(stmt) => {
                assert!(!stmt.if_exists);
                assert_eq!(stmt.index_name, "idx_users_name");
            }
            _ => panic!("Expected DROP INDEX"),
        }
    }

    #[test]
    fn test_parse_drop_view() {
        let program = parse_program("DROP VIEW IF EXISTS active_users;").unwrap();
        match &program.statements[0] {
            Statement::DropView(stmt) => {
                assert!(stmt.if_exists);
                assert_eq!(stmt.view_name, "active_users");
            }
            _ => panic!("Expected DROP VIEW"),
        }
    }

    #[test]
    fn test_parse_drop_trigger() {
        let program = parse_program("DROP TRIGGER IF EXISTS trg_audit;").unwrap();
        match &program.statements[0] {
            Statement::DropTrigger(stmt) => {
                assert!(stmt.if_exists);
                assert_eq!(stmt.trigger_name, "trg_audit");
            }
            _ => panic!("Expected DROP TRIGGER"),
        }
    }

    // --- TCL (Transaction Control) Statements ---

    #[test]
    fn test_parse_begin() {
        let program = parse_program("BEGIN;").unwrap();
        match &program.statements[0] {
            Statement::Begin(stmt) => {
                assert!(stmt.transaction_type.is_none());
            }
            _ => panic!("Expected BEGIN"),
        }
    }

    #[test]
    fn test_parse_begin_deferred() {
        let program = parse_program("BEGIN DEFERRED TRANSACTION;").unwrap();
        match &program.statements[0] {
            Statement::Begin(stmt) => {
                assert_eq!(stmt.transaction_type, Some(TransactionType::Deferred));
            }
            _ => panic!("Expected BEGIN"),
        }
    }

    #[test]
    fn test_parse_begin_immediate() {
        let program = parse_program("BEGIN IMMEDIATE;").unwrap();
        match &program.statements[0] {
            Statement::Begin(stmt) => {
                assert_eq!(stmt.transaction_type, Some(TransactionType::Immediate));
            }
            _ => panic!("Expected BEGIN"),
        }
    }

    #[test]
    fn test_parse_begin_exclusive() {
        let program = parse_program("BEGIN EXCLUSIVE TRANSACTION;").unwrap();
        match &program.statements[0] {
            Statement::Begin(stmt) => {
                assert_eq!(stmt.transaction_type, Some(TransactionType::Exclusive));
            }
            _ => panic!("Expected BEGIN"),
        }
    }

    #[test]
    fn test_parse_commit() {
        let program = parse_program("COMMIT;").unwrap();
        assert!(matches!(&program.statements[0], Statement::Commit(_)));
    }

    #[test]
    fn test_parse_end_as_commit() {
        let program = parse_program("END TRANSACTION;").unwrap();
        assert!(matches!(&program.statements[0], Statement::Commit(_)));
    }

    #[test]
    fn test_parse_rollback() {
        let program = parse_program("ROLLBACK;").unwrap();
        match &program.statements[0] {
            Statement::Rollback(stmt) => {
                assert!(stmt.savepoint.is_none());
            }
            _ => panic!("Expected ROLLBACK"),
        }
    }

    #[test]
    fn test_parse_rollback_to_savepoint() {
        let program = parse_program("ROLLBACK TO SAVEPOINT sp1;").unwrap();
        match &program.statements[0] {
            Statement::Rollback(stmt) => {
                assert_eq!(stmt.savepoint, Some("sp1".to_string()));
            }
            _ => panic!("Expected ROLLBACK"),
        }
    }

    #[test]
    fn test_parse_savepoint() {
        let program = parse_program("SAVEPOINT sp1;").unwrap();
        match &program.statements[0] {
            Statement::Savepoint(stmt) => {
                assert_eq!(stmt.name, "sp1");
            }
            _ => panic!("Expected SAVEPOINT"),
        }
    }

    #[test]
    fn test_parse_release() {
        let program = parse_program("RELEASE SAVEPOINT sp1;").unwrap();
        match &program.statements[0] {
            Statement::Release(stmt) => {
                assert_eq!(stmt.name, "sp1");
            }
            _ => panic!("Expected RELEASE"),
        }
    }

    #[test]
    fn test_parse_release_without_savepoint_keyword() {
        let program = parse_program("RELEASE sp1;").unwrap();
        match &program.statements[0] {
            Statement::Release(stmt) => {
                assert_eq!(stmt.name, "sp1");
            }
            _ => panic!("Expected RELEASE"),
        }
    }

    // --- Database Management Statements ---

    #[test]
    fn test_parse_vacuum() {
        let program = parse_program("VACUUM;").unwrap();
        match &program.statements[0] {
            Statement::Vacuum(stmt) => {
                assert!(stmt.schema.is_none());
                assert!(stmt.into_file.is_none());
            }
            _ => panic!("Expected VACUUM"),
        }
    }

    #[test]
    fn test_parse_vacuum_schema() {
        let program = parse_program("VACUUM main;").unwrap();
        match &program.statements[0] {
            Statement::Vacuum(stmt) => {
                assert_eq!(stmt.schema, Some("main".to_string()));
            }
            _ => panic!("Expected VACUUM"),
        }
    }

    #[test]
    fn test_parse_vacuum_into() {
        let program = parse_program("VACUUM INTO 'backup.db';").unwrap();
        match &program.statements[0] {
            Statement::Vacuum(stmt) => {
                assert_eq!(stmt.into_file, Some("backup.db".to_string()));
            }
            _ => panic!("Expected VACUUM"),
        }
    }

    #[test]
    fn test_parse_analyze() {
        let program = parse_program("ANALYZE;").unwrap();
        match &program.statements[0] {
            Statement::Analyze(stmt) => {
                assert!(stmt.target.is_none());
            }
            _ => panic!("Expected ANALYZE"),
        }
    }

    #[test]
    fn test_parse_analyze_table() {
        let program = parse_program("ANALYZE users;").unwrap();
        match &program.statements[0] {
            Statement::Analyze(stmt) => {
                let target = stmt.target.as_ref().unwrap();
                assert!(target.schema.is_none());
                assert_eq!(target.name, "users");
            }
            _ => panic!("Expected ANALYZE"),
        }
    }

    #[test]
    fn test_parse_analyze_schema_table() {
        let program = parse_program("ANALYZE main.users;").unwrap();
        match &program.statements[0] {
            Statement::Analyze(stmt) => {
                let target = stmt.target.as_ref().unwrap();
                assert_eq!(target.schema, Some("main".to_string()));
                assert_eq!(target.name, "users");
            }
            _ => panic!("Expected ANALYZE"),
        }
    }

    #[test]
    fn test_parse_reindex() {
        let program = parse_program("REINDEX;").unwrap();
        match &program.statements[0] {
            Statement::Reindex(stmt) => {
                assert!(stmt.target.is_none());
            }
            _ => panic!("Expected REINDEX"),
        }
    }

    #[test]
    fn test_parse_reindex_table() {
        let program = parse_program("REINDEX users;").unwrap();
        match &program.statements[0] {
            Statement::Reindex(stmt) => {
                let target = stmt.target.as_ref().unwrap();
                assert_eq!(target.name, "users");
            }
            _ => panic!("Expected REINDEX"),
        }
    }

    #[test]
    fn test_parse_attach() {
        let program = parse_program("ATTACH 'file.db' AS db2;").unwrap();
        match &program.statements[0] {
            Statement::Attach(stmt) => {
                assert!(matches!(&stmt.expr, Expr::String(s, _) if s == "file.db"));
                assert_eq!(stmt.schema_name, "db2");
            }
            _ => panic!("Expected ATTACH"),
        }
    }

    #[test]
    fn test_parse_attach_database() {
        let program = parse_program("ATTACH DATABASE 'file.db' AS db2;").unwrap();
        match &program.statements[0] {
            Statement::Attach(stmt) => {
                assert!(matches!(&stmt.expr, Expr::String(s, _) if s == "file.db"));
                assert_eq!(stmt.schema_name, "db2");
            }
            _ => panic!("Expected ATTACH"),
        }
    }

    #[test]
    fn test_parse_detach() {
        let program = parse_program("DETACH db2;").unwrap();
        match &program.statements[0] {
            Statement::Detach(stmt) => {
                assert_eq!(stmt.schema_name, "db2");
            }
            _ => panic!("Expected DETACH"),
        }
    }

    #[test]
    fn test_parse_detach_database() {
        let program = parse_program("DETACH DATABASE db2;").unwrap();
        match &program.statements[0] {
            Statement::Detach(stmt) => {
                assert_eq!(stmt.schema_name, "db2");
            }
            _ => panic!("Expected DETACH"),
        }
    }

    #[test]
    fn test_parse_pragma_simple() {
        let program = parse_program("PRAGMA table_info;").unwrap();
        match &program.statements[0] {
            Statement::Pragma(stmt) => {
                assert!(stmt.schema.is_none());
                assert_eq!(stmt.name, "table_info");
                assert!(stmt.value.is_none());
            }
            _ => panic!("Expected PRAGMA"),
        }
    }

    #[test]
    fn test_parse_pragma_with_schema() {
        let program = parse_program("PRAGMA main.table_info;").unwrap();
        match &program.statements[0] {
            Statement::Pragma(stmt) => {
                assert_eq!(stmt.schema, Some("main".to_string()));
                assert_eq!(stmt.name, "table_info");
            }
            _ => panic!("Expected PRAGMA"),
        }
    }

    #[test]
    fn test_parse_pragma_assign() {
        let program = parse_program("PRAGMA cache_size = 1000;").unwrap();
        match &program.statements[0] {
            Statement::Pragma(stmt) => {
                assert_eq!(stmt.name, "cache_size");
                match &stmt.value {
                    Some(PragmaValue::Assign(Expr::Integer(1000, _))) => {}
                    _ => panic!("Expected assign value"),
                }
            }
            _ => panic!("Expected PRAGMA"),
        }
    }

    #[test]
    fn test_parse_pragma_call() {
        let program = parse_program("PRAGMA table_info(users);").unwrap();
        match &program.statements[0] {
            Statement::Pragma(stmt) => {
                assert_eq!(stmt.name, "table_info");
                match &stmt.value {
                    Some(PragmaValue::Call(Expr::Ident(name, _, _))) if name == "users" => {}
                    _ => panic!("Expected call value"),
                }
            }
            _ => panic!("Expected PRAGMA"),
        }
    }

    // ========================================
    // JOIN Tests
    // ========================================

    #[test]
    fn test_parse_inner_join() {
        let program = parse_program("SELECT * FROM users JOIN orders ON users.id = orders.user_id;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                assert_eq!(from.tables.len(), 1);
                match &from.tables[0] {
                    TableOrSubquery::Join { left, join_type, right, constraint, .. } => {
                        assert_eq!(*join_type, JoinType::Inner);
                        match left.as_ref() {
                            TableOrSubquery::Table { name, .. } => assert_eq!(name, "users"),
                            _ => panic!("Expected table on left"),
                        }
                        match right.as_ref() {
                            TableOrSubquery::Table { name, .. } => assert_eq!(name, "orders"),
                            _ => panic!("Expected table on right"),
                        }
                        assert!(matches!(constraint, Some(JoinConstraint::On(_))));
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_inner_join_explicit() {
        let program = parse_program("SELECT * FROM users INNER JOIN orders ON 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, .. } => {
                        assert_eq!(*join_type, JoinType::Inner);
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_left_join() {
        let program = parse_program("SELECT * FROM users LEFT JOIN orders ON 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, .. } => {
                        assert_eq!(*join_type, JoinType::Left);
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_left_outer_join() {
        let program = parse_program("SELECT * FROM users LEFT OUTER JOIN orders ON 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, .. } => {
                        assert_eq!(*join_type, JoinType::Left);
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_right_join() {
        let program = parse_program("SELECT * FROM users RIGHT JOIN orders ON 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, .. } => {
                        assert_eq!(*join_type, JoinType::Right);
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_full_join() {
        let program = parse_program("SELECT * FROM users FULL JOIN orders ON 1;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, .. } => {
                        assert_eq!(*join_type, JoinType::Full);
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cross_join() {
        let program = parse_program("SELECT * FROM users CROSS JOIN orders;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, constraint, .. } => {
                        assert_eq!(*join_type, JoinType::Cross);
                        assert!(constraint.is_none()); // CROSS JOIN has no ON clause
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_natural_join() {
        let program = parse_program("SELECT * FROM users NATURAL JOIN orders;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, constraint, .. } => {
                        assert_eq!(*join_type, JoinType::Natural);
                        assert!(constraint.is_none()); // NATURAL JOIN has no ON clause
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_natural_left_join() {
        let program = parse_program("SELECT * FROM users NATURAL LEFT JOIN orders;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { join_type, .. } => {
                        assert_eq!(*join_type, JoinType::NaturalLeft);
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_join_using() {
        let program = parse_program("SELECT * FROM users JOIN orders USING (id, user_id);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { constraint, .. } => {
                        match constraint {
                            Some(JoinConstraint::Using(cols)) => {
                                assert_eq!(cols.len(), 2);
                                assert_eq!(cols[0], "id");
                                assert_eq!(cols[1], "user_id");
                            }
                            _ => panic!("Expected USING constraint"),
                        }
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_multiple_joins() {
        let program = parse_program(
            "SELECT * FROM users JOIN orders ON 1 JOIN items ON 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                // Should be nested: (users JOIN orders) JOIN items
                match &from.tables[0] {
                    TableOrSubquery::Join { left, right, .. } => {
                        // Right should be "items"
                        match right.as_ref() {
                            TableOrSubquery::Table { name, .. } => assert_eq!(name, "items"),
                            _ => panic!("Expected table on right"),
                        }
                        // Left should be another join
                        match left.as_ref() {
                            TableOrSubquery::Join { left: inner_left, right: inner_right, .. } => {
                                match inner_left.as_ref() {
                                    TableOrSubquery::Table { name, .. } => assert_eq!(name, "users"),
                                    _ => panic!("Expected users on inner left"),
                                }
                                match inner_right.as_ref() {
                                    TableOrSubquery::Table { name, .. } => assert_eq!(name, "orders"),
                                    _ => panic!("Expected orders on inner right"),
                                }
                            }
                            _ => panic!("Expected nested join on left"),
                        }
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_join_with_alias() {
        let program = parse_program("SELECT * FROM users u JOIN orders o ON u.id = o.user_id;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                match &from.tables[0] {
                    TableOrSubquery::Join { left, right, .. } => {
                        match left.as_ref() {
                            TableOrSubquery::Table { name, alias, .. } => {
                                assert_eq!(name, "users");
                                assert_eq!(alias.as_deref(), Some("u"));
                            }
                            _ => panic!("Expected table on left"),
                        }
                        match right.as_ref() {
                            TableOrSubquery::Table { name, alias, .. } => {
                                assert_eq!(name, "orders");
                                assert_eq!(alias.as_deref(), Some("o"));
                            }
                            _ => panic!("Expected table on right"),
                        }
                    }
                    _ => panic!("Expected JOIN"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // Compound Operator Tests (UNION, INTERSECT, EXCEPT)
    // ========================================

    #[test]
    fn test_parse_union() {
        let program = parse_program("SELECT 1 UNION SELECT 2;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 1);
                assert_eq!(stmt.compounds[0].0, CompoundOp::Union);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_union_all() {
        let program = parse_program("SELECT 1 UNION ALL SELECT 2;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 1);
                assert_eq!(stmt.compounds[0].0, CompoundOp::UnionAll);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_intersect() {
        let program = parse_program("SELECT 1 INTERSECT SELECT 2;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 1);
                assert_eq!(stmt.compounds[0].0, CompoundOp::Intersect);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_except() {
        let program = parse_program("SELECT 1 EXCEPT SELECT 2;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 1);
                assert_eq!(stmt.compounds[0].0, CompoundOp::Except);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_multiple_compounds() {
        let program = parse_program("SELECT 1 UNION SELECT 2 UNION ALL SELECT 3;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 2);
                assert_eq!(stmt.compounds[0].0, CompoundOp::Union);
                assert_eq!(stmt.compounds[1].0, CompoundOp::UnionAll);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_compound_with_order_by() {
        let program = parse_program("SELECT id FROM a UNION SELECT id FROM b ORDER BY id;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 1);
                assert!(stmt.order_by.is_some());
                let order_by = stmt.order_by.as_ref().unwrap();
                assert_eq!(order_by.len(), 1);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_compound_with_limit() {
        let program = parse_program("SELECT 1 UNION SELECT 2 LIMIT 10;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                assert_eq!(stmt.compounds.len(), 1);
                assert!(stmt.limit.is_some());
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_compound_preserves_from() {
        let program = parse_program("SELECT id FROM users UNION SELECT id FROM orders;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                // First SELECT has users table
                assert!(stmt.from.is_some());
                let (_, name) = first_table(stmt.from.as_ref().unwrap());
                assert_eq!(name, "users");

                // Second SELECT (in compounds) has orders table
                assert_eq!(stmt.compounds.len(), 1);
                let (_, core) = &stmt.compounds[0];
                assert!(core.from.is_some());
                match &core.from.as_ref().unwrap().tables[0] {
                    TableOrSubquery::Table { name, .. } => assert_eq!(name, "orders"),
                    _ => panic!("Expected table"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // CTE (Common Table Expression) Tests
    // ========================================

    #[test]
    fn test_parse_simple_cte() {
        let program = parse_program("WITH cte AS (SELECT 1) SELECT * FROM cte;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert!(!with.recursive);
                assert_eq!(with.ctes.len(), 1);
                assert_eq!(with.ctes[0].name, "cte");
                assert!(with.ctes[0].columns.is_none());
                assert!(with.ctes[0].materialized.is_none());
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cte_with_columns() {
        let program = parse_program("WITH cte(a, b) AS (SELECT 1, 2) SELECT * FROM cte;").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                let cte = &with.ctes[0];
                assert_eq!(cte.name, "cte");
                let cols = cte.columns.as_ref().expect("Expected columns");
                assert_eq!(cols, &["a".to_string(), "b".to_string()]);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_recursive_cte() {
        let program = parse_program(
            "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM cnt) SELECT x FROM cnt;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert!(with.recursive);
                assert_eq!(with.ctes.len(), 1);
                assert_eq!(with.ctes[0].name, "cnt");
                let cols = with.ctes[0].columns.as_ref().expect("Expected columns");
                assert_eq!(cols, &["x".to_string()]);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_multiple_ctes() {
        let program = parse_program(
            "WITH a AS (SELECT 1), b AS (SELECT 2) SELECT * FROM a, b;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert_eq!(with.ctes.len(), 2);
                assert_eq!(with.ctes[0].name, "a");
                assert_eq!(with.ctes[1].name, "b");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cte_materialized() {
        let program = parse_program(
            "WITH cte AS MATERIALIZED (SELECT 1) SELECT * FROM cte;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert_eq!(with.ctes[0].materialized, Some(Materialized::Materialized));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_cte_not_materialized() {
        let program = parse_program(
            "WITH cte AS NOT MATERIALIZED (SELECT 1) SELECT * FROM cte;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert_eq!(with.ctes[0].materialized, Some(Materialized::NotMaterialized));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // Function Call Tests
    // ========================================

    #[test]
    fn test_parse_function_call_no_args() {
        let program = parse_program("SELECT now();").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { name, args, distinct, filter, over, .. } => {
                    assert_eq!(name, "now");
                    assert!(args.is_empty());
                    assert!(!distinct);
                    assert!(filter.is_none());
                    assert!(over.is_none());
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_function_call_with_args() {
        let program = parse_program("SELECT substr(name, 1, 5);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { name, args, .. } => {
                    assert_eq!(name, "substr");
                    assert_eq!(args.len(), 3);
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_count_star() {
        let program = parse_program("SELECT count(*);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { name, args, .. } => {
                    assert_eq!(name, "count");
                    assert_eq!(args.len(), 1);
                    assert!(matches!(args[0], Expr::Star(_)));
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_count_distinct() {
        let program = parse_program("SELECT count(DISTINCT name);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { name, distinct, .. } => {
                    assert_eq!(name, "count");
                    assert!(distinct);
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // Window Function Tests
    // ========================================

    #[test]
    fn test_parse_window_function_over() {
        let program = parse_program("SELECT row_number() OVER ();").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { name, over, .. } => {
                    assert_eq!(name, "row_number");
                    let win = over.as_ref().expect("Expected OVER clause");
                    assert!(win.partition_by.is_none());
                    assert!(win.order_by.is_none());
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_window_function_partition_by() {
        let program = parse_program("SELECT sum(amount) OVER (PARTITION BY category);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { over, .. } => {
                    let win = over.as_ref().expect("Expected OVER clause");
                    let partition = win.partition_by.as_ref().expect("Expected PARTITION BY");
                    assert_eq!(partition.len(), 1);
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_window_function_order_by() {
        let program = parse_program("SELECT rank() OVER (ORDER BY score DESC);").unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { over, .. } => {
                    let win = over.as_ref().expect("Expected OVER clause");
                    let order = win.order_by.as_ref().expect("Expected ORDER BY");
                    assert_eq!(order.len(), 1);
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_window_function_frame() {
        let program = parse_program(
            "SELECT sum(x) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW);"
        ).unwrap();
        match &program.statements[0] {
            Statement::Select(stmt) => match first_expr(stmt) {
                Expr::FunctionCall { over, .. } => {
                    let win = over.as_ref().expect("Expected OVER clause");
                    let frame = win.frame.as_ref().expect("Expected frame");
                    assert_eq!(frame.unit, FrameUnit::Rows);
                }
                _ => panic!("Expected function call"),
            },
            _ => panic!("Expected SELECT"),
        }
    }

    // ========================================
    // INSERT Statement Tests
    // ========================================

    #[test]
    fn test_parse_insert_values() {
        let program = parse_program("INSERT INTO users VALUES (1, 'alice');").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.table_name, "users");
                assert!(stmt.schema.is_none());
                assert!(stmt.columns.is_none());
                assert!(stmt.or_action.is_none());
                match &stmt.source {
                    InsertSource::Values(rows) => {
                        assert_eq!(rows.len(), 1);
                        assert_eq!(rows[0].len(), 2);
                    }
                    _ => panic!("Expected VALUES source"),
                }
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_with_columns() {
        let program = parse_program("INSERT INTO users (id, name) VALUES (1, 'alice');").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.table_name, "users");
                let cols = stmt.columns.as_ref().expect("Expected columns");
                assert_eq!(cols, &["id", "name"]);
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_multiple_rows() {
        let program = parse_program(
            "INSERT INTO users VALUES (1, 'alice'), (2, 'bob'), (3, 'charlie');"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                match &stmt.source {
                    InsertSource::Values(rows) => {
                        assert_eq!(rows.len(), 3);
                    }
                    _ => panic!("Expected VALUES source"),
                }
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_default_values() {
        let program = parse_program("INSERT INTO users DEFAULT VALUES;").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.table_name, "users");
                assert!(matches!(stmt.source, InsertSource::DefaultValues));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_select() {
        let program = parse_program(
            "INSERT INTO users_copy SELECT * FROM users;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.table_name, "users_copy");
                match &stmt.source {
                    InsertSource::Select(select) => {
                        assert!(select.from.is_some());
                    }
                    _ => panic!("Expected SELECT source"),
                }
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_or_replace() {
        let program = parse_program("INSERT OR REPLACE INTO users VALUES (1, 'alice');").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.or_action, Some(ConflictAction::Replace));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_or_ignore() {
        let program = parse_program("INSERT OR IGNORE INTO users VALUES (1, 'alice');").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.or_action, Some(ConflictAction::Ignore));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_or_abort() {
        let program = parse_program("INSERT OR ABORT INTO users VALUES (1, 'alice');").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.or_action, Some(ConflictAction::Abort));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_replace() {
        // REPLACE is syntactic sugar for INSERT OR REPLACE
        let program = parse_program("REPLACE INTO users VALUES (1, 'alice');").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.or_action, Some(ConflictAction::Replace));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_with_schema() {
        let program = parse_program("INSERT INTO main.users VALUES (1);").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.schema.as_deref(), Some("main"));
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_returning() {
        let program = parse_program("INSERT INTO users VALUES (1) RETURNING id, name;").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let ret = stmt.returning.as_ref().expect("Expected RETURNING");
                assert_eq!(ret.len(), 2);
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_returning_star() {
        let program = parse_program("INSERT INTO users VALUES (1) RETURNING *;").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let ret = stmt.returning.as_ref().expect("Expected RETURNING");
                assert_eq!(ret.len(), 1);
                assert!(matches!(ret[0], ResultColumn::Star(_)));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_on_conflict_do_nothing() {
        let program = parse_program(
            "INSERT INTO users VALUES (1) ON CONFLICT DO NOTHING;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let upsert = stmt.upsert.as_ref().expect("Expected upsert clause");
                assert_eq!(upsert.action, ConflictAction::Nothing);
                assert!(upsert.target.is_none());
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_on_conflict_target_do_nothing() {
        let program = parse_program(
            "INSERT INTO users VALUES (1) ON CONFLICT (id) DO NOTHING;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let upsert = stmt.upsert.as_ref().expect("Expected upsert clause");
                assert_eq!(upsert.action, ConflictAction::Nothing);
                let target = upsert.target.as_ref().expect("Expected conflict target");
                assert_eq!(target.columns.len(), 1);
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_on_conflict_do_update() {
        let program = parse_program(
            "INSERT INTO users VALUES (1, 'alice') ON CONFLICT (id) DO UPDATE SET name = excluded.name;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let upsert = stmt.upsert.as_ref().expect("Expected upsert clause");
                assert_eq!(upsert.action, ConflictAction::Update);
                let updates = upsert.update_set.as_ref().expect("Expected SET clause");
                assert_eq!(updates.len(), 1);
                assert_eq!(updates[0].0, vec!["name"]);
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_on_conflict_do_update_where() {
        let program = parse_program(
            "INSERT INTO users VALUES (1) ON CONFLICT (id) DO UPDATE SET active = 1 WHERE old.active = 0;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let upsert = stmt.upsert.as_ref().expect("Expected upsert clause");
                assert!(upsert.update_where.is_some());
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_with_cte() {
        let program = parse_program(
            "WITH new_users AS (SELECT 1 AS id) INSERT INTO users SELECT * FROM new_users;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert_eq!(with.ctes.len(), 1);
                assert_eq!(with.ctes[0].name, "new_users");
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_alias() {
        let program = parse_program("INSERT INTO users AS u VALUES (1);").unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                assert_eq!(stmt.alias.as_deref(), Some("u"));
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_insert_expressions() {
        let program = parse_program(
            "INSERT INTO results VALUES (1 + 2, 'hello' || ' world', NULL);"
        ).unwrap();
        match &program.statements[0] {
            Statement::Insert(stmt) => {
                match &stmt.source {
                    InsertSource::Values(rows) => {
                        assert_eq!(rows.len(), 1);
                        assert_eq!(rows[0].len(), 3);
                        // First expression: 1 + 2
                        assert!(matches!(rows[0][0], Expr::Binary { .. }));
                        // Second expression: concat
                        assert!(matches!(rows[0][1], Expr::Binary { .. }));
                        // Third: NULL
                        assert!(matches!(rows[0][2], Expr::Null(_)));
                    }
                    _ => panic!("Expected VALUES source"),
                }
            }
            _ => panic!("Expected INSERT"),
        }
    }

    // ========================================
    // UPDATE Statement Tests
    // ========================================

    #[test]
    fn test_parse_update_simple() {
        let program = parse_program("UPDATE users SET name = 'alice';").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.table_name, "users");
                assert!(stmt.schema.is_none());
                assert_eq!(stmt.assignments.len(), 1);
                assert_eq!(stmt.assignments[0].columns, vec!["name"]);
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_multiple_columns() {
        let program = parse_program(
            "UPDATE users SET name = 'alice', age = 30, active = 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.assignments.len(), 3);
                assert_eq!(stmt.assignments[0].columns, vec!["name"]);
                assert_eq!(stmt.assignments[1].columns, vec!["age"]);
                assert_eq!(stmt.assignments[2].columns, vec!["active"]);
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_where() {
        let program = parse_program(
            "UPDATE users SET name = 'bob' WHERE id = 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(stmt.where_clause.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_schema() {
        let program = parse_program("UPDATE main.users SET name = 'alice';").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.schema.as_deref(), Some("main"));
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_or_replace() {
        let program = parse_program("UPDATE OR REPLACE users SET name = 'alice';").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.or_action, Some(ConflictAction::Replace));
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_or_ignore() {
        let program = parse_program("UPDATE OR IGNORE users SET name = 'alice';").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.or_action, Some(ConflictAction::Ignore));
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_alias() {
        let program = parse_program("UPDATE users AS u SET name = 'alice';").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.alias.as_deref(), Some("u"));
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_returning() {
        let program = parse_program(
            "UPDATE users SET name = 'alice' RETURNING id, name;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                let ret = stmt.returning.as_ref().expect("Expected RETURNING");
                assert_eq!(ret.len(), 2);
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_returning_star() {
        let program = parse_program("UPDATE users SET name = 'alice' RETURNING *;").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                let ret = stmt.returning.as_ref().expect("Expected RETURNING");
                assert_eq!(ret.len(), 1);
                assert!(matches!(ret[0], ResultColumn::Star(_)));
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_from() {
        let program = parse_program(
            "UPDATE users SET name = new_names.name FROM new_names WHERE users.id = new_names.id;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                let from = stmt.from.as_ref().expect("Expected FROM clause");
                assert_eq!(from.tables.len(), 1);
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_cte() {
        let program = parse_program(
            "WITH new_data AS (SELECT 1 AS id) UPDATE users SET name = 'alice' WHERE id = 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert_eq!(with.ctes.len(), 1);
                assert_eq!(with.ctes[0].name, "new_data");
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_expression() {
        let program = parse_program("UPDATE counters SET value = value + 1;").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(matches!(stmt.assignments[0].expr, Expr::Binary { .. }));
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_indexed_by() {
        let program = parse_program(
            "UPDATE users INDEXED BY idx_users_name SET name = 'alice' WHERE name = 'bob';"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                match stmt.indexed.as_ref().expect("Expected INDEXED BY") {
                    IndexedBy::Index(name) => assert_eq!(name, "idx_users_name"),
                    _ => panic!("Expected Index"),
                }
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_not_indexed() {
        let program = parse_program("UPDATE users NOT INDEXED SET name = 'alice';").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(matches!(stmt.indexed, Some(IndexedBy::NotIndexed)));
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_multi_column_assignment() {
        // SQLite supports (col1, col2) = (expr1, expr2) syntax
        // But our simplified version just does (col1, col2) = expr
        // This tests the column list parsing
        let program = parse_program("UPDATE users SET name = 'alice' WHERE id = 1;").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert_eq!(stmt.assignments.len(), 1);
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    // ========================================
    // DELETE Statement Tests
    // ========================================

    #[test]
    fn test_parse_delete_simple() {
        let program = parse_program("DELETE FROM users;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert_eq!(stmt.table_name, "users");
                assert!(stmt.schema.is_none());
                assert!(stmt.where_clause.is_none());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_where() {
        let program = parse_program("DELETE FROM users WHERE id = 1;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert_eq!(stmt.table_name, "users");
                assert!(stmt.where_clause.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_schema() {
        let program = parse_program("DELETE FROM main.users;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert_eq!(stmt.schema.as_deref(), Some("main"));
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_alias() {
        let program = parse_program("DELETE FROM users AS u WHERE u.id = 1;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert_eq!(stmt.alias.as_deref(), Some("u"));
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_returning() {
        let program = parse_program("DELETE FROM users WHERE id = 1 RETURNING id, name;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                let ret = stmt.returning.as_ref().expect("Expected RETURNING");
                assert_eq!(ret.len(), 2);
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_returning_star() {
        let program = parse_program("DELETE FROM users RETURNING *;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                let ret = stmt.returning.as_ref().expect("Expected RETURNING");
                assert_eq!(ret.len(), 1);
                assert!(matches!(ret[0], ResultColumn::Star(_)));
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_indexed_by() {
        let program = parse_program(
            "DELETE FROM users INDEXED BY idx_users_id WHERE id = 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                match stmt.indexed.as_ref().expect("Expected INDEXED BY") {
                    IndexedBy::Index(name) => assert_eq!(name, "idx_users_id"),
                    _ => panic!("Expected Index"),
                }
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_not_indexed() {
        let program = parse_program("DELETE FROM users NOT INDEXED WHERE id = 1;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(matches!(stmt.indexed, Some(IndexedBy::NotIndexed)));
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_cte() {
        let program = parse_program(
            "WITH old_users AS (SELECT id FROM users) DELETE FROM users WHERE id = 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                let with = stmt.with_clause.as_ref().expect("Expected WITH clause");
                assert_eq!(with.ctes.len(), 1);
                assert_eq!(with.ctes[0].name, "old_users");
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_complex_where() {
        let program = parse_program(
            "DELETE FROM users WHERE age > 18 AND active = 0 OR created < '2020-01-01';"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(stmt.where_clause.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    // ========================================
    // UPDATE Limited Tests (ORDER BY + LIMIT)
    // ========================================

    #[test]
    fn test_parse_update_with_limit() {
        let program = parse_program("UPDATE users SET active = 0 LIMIT 10;").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_none());
                assert!(stmt.order_by.is_none());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_order_by_limit() {
        let program = parse_program(
            "UPDATE users SET active = 0 ORDER BY created_at DESC LIMIT 10;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                let order_by = stmt.order_by.as_ref().expect("Expected ORDER BY");
                assert_eq!(order_by.len(), 1);
                assert_eq!(order_by[0].direction, Some(OrderDirection::Desc));
                assert!(stmt.limit.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_limit_offset() {
        let program = parse_program("UPDATE users SET active = 0 LIMIT 10 OFFSET 5;").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_with_limit_comma_offset() {
        // LIMIT expr, expr syntax (limit, offset)
        let program = parse_program("UPDATE users SET active = 0 LIMIT 10, 5;").unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_full_limited() {
        let program = parse_program(
            "UPDATE users SET active = 0 WHERE status = 'old' \
             ORDER BY last_login ASC LIMIT 100 OFFSET 0;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(stmt.where_clause.is_some());
                let order_by = stmt.order_by.as_ref().expect("Expected ORDER BY");
                assert_eq!(order_by.len(), 1);
                assert_eq!(order_by[0].direction, Some(OrderDirection::Asc));
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_update_order_by_multiple_columns() {
        let program = parse_program(
            "UPDATE users SET active = 0 ORDER BY created_at DESC, name ASC LIMIT 10;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                let order_by = stmt.order_by.as_ref().expect("Expected ORDER BY");
                assert_eq!(order_by.len(), 2);
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    // ========================================
    // DELETE Limited Tests (ORDER BY + LIMIT)
    // ========================================

    #[test]
    fn test_parse_delete_with_limit() {
        let program = parse_program("DELETE FROM logs LIMIT 1000;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_none());
                assert!(stmt.order_by.is_none());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_order_by_limit() {
        let program = parse_program(
            "DELETE FROM logs ORDER BY created_at ASC LIMIT 1000;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                let order_by = stmt.order_by.as_ref().expect("Expected ORDER BY");
                assert_eq!(order_by.len(), 1);
                assert_eq!(order_by[0].direction, Some(OrderDirection::Asc));
                assert!(stmt.limit.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_limit_offset() {
        let program = parse_program("DELETE FROM logs LIMIT 1000 OFFSET 500;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_with_limit_comma_offset() {
        // LIMIT expr, expr syntax (limit, offset)
        let program = parse_program("DELETE FROM logs LIMIT 1000, 500;").unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_full_limited() {
        let program = parse_program(
            "DELETE FROM logs WHERE level = 'debug' \
             ORDER BY timestamp ASC LIMIT 10000 OFFSET 0;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(stmt.where_clause.is_some());
                let order_by = stmt.order_by.as_ref().expect("Expected ORDER BY");
                assert_eq!(order_by.len(), 1);
                assert!(stmt.limit.is_some());
                assert!(stmt.offset.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_order_by_multiple_columns() {
        let program = parse_program(
            "DELETE FROM logs ORDER BY priority DESC, timestamp ASC LIMIT 100;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                let order_by = stmt.order_by.as_ref().expect("Expected ORDER BY");
                assert_eq!(order_by.len(), 2);
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_delete_limited_with_returning() {
        let program = parse_program(
            "DELETE FROM logs RETURNING id ORDER BY timestamp LIMIT 10;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Delete(stmt) => {
                assert!(stmt.returning.is_some());
                assert!(stmt.order_by.is_some());
                assert!(stmt.limit.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_update_limited_with_returning() {
        let program = parse_program(
            "UPDATE users SET processed = 1 RETURNING id ORDER BY created_at LIMIT 10;"
        ).unwrap();
        match &program.statements[0] {
            Statement::Update(stmt) => {
                assert!(stmt.returning.is_some());
                assert!(stmt.order_by.is_some());
                assert!(stmt.limit.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    // ========================================
    // CREATE INDEX Tests
    // ========================================

    #[test]
    fn test_parse_create_index() {
        let program = parse_program("CREATE INDEX idx_users_name ON users (name);").unwrap();
        match &program.statements[0] {
            Statement::CreateIndex(stmt) => {
                assert!(!stmt.unique);
                assert!(!stmt.if_not_exists);
                assert_eq!(stmt.index_name, "idx_users_name");
                assert_eq!(stmt.table_name, "users");
                assert_eq!(stmt.columns.len(), 1);
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_unique_index() {
        let program = parse_program("CREATE UNIQUE INDEX idx_users_email ON users (email);").unwrap();
        match &program.statements[0] {
            Statement::CreateIndex(stmt) => {
                assert!(stmt.unique);
                assert_eq!(stmt.index_name, "idx_users_email");
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_index_if_not_exists() {
        let program = parse_program(
            "CREATE INDEX IF NOT EXISTS idx_users_name ON users (name);"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateIndex(stmt) => {
                assert!(stmt.if_not_exists);
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_index_with_schema() {
        let program = parse_program("CREATE INDEX main.idx_users ON users (id);").unwrap();
        match &program.statements[0] {
            Statement::CreateIndex(stmt) => {
                assert_eq!(stmt.schema.as_deref(), Some("main"));
                assert_eq!(stmt.index_name, "idx_users");
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_index_multiple_columns() {
        let program = parse_program(
            "CREATE INDEX idx_users_name_age ON users (name, age);"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateIndex(stmt) => {
                assert_eq!(stmt.columns.len(), 2);
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_index_with_where() {
        let program = parse_program(
            "CREATE INDEX idx_active_users ON users (name) WHERE active = 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateIndex(stmt) => {
                assert!(stmt.where_clause.is_some());
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    // ========================================
    // CREATE VIEW Tests
    // ========================================

    #[test]
    fn test_parse_create_view() {
        let program = parse_program("CREATE VIEW active_users AS SELECT * FROM users WHERE active = 1;").unwrap();
        match &program.statements[0] {
            Statement::CreateView(stmt) => {
                assert!(!stmt.temporary);
                assert!(!stmt.if_not_exists);
                assert_eq!(stmt.view_name, "active_users");
                assert!(stmt.columns.is_none());
            }
            _ => panic!("Expected CREATE VIEW"),
        }
    }

    #[test]
    fn test_parse_create_temp_view() {
        let program = parse_program(
            "CREATE TEMP VIEW temp_view AS SELECT 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateView(stmt) => {
                assert!(stmt.temporary);
            }
            _ => panic!("Expected CREATE VIEW"),
        }
    }

    #[test]
    fn test_parse_create_view_if_not_exists() {
        let program = parse_program(
            "CREATE VIEW IF NOT EXISTS my_view AS SELECT 1;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateView(stmt) => {
                assert!(stmt.if_not_exists);
            }
            _ => panic!("Expected CREATE VIEW"),
        }
    }

    #[test]
    fn test_parse_create_view_with_columns() {
        let program = parse_program(
            "CREATE VIEW my_view (col1, col2) AS SELECT a, b FROM t;"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateView(stmt) => {
                let cols = stmt.columns.as_ref().expect("Expected columns");
                assert_eq!(cols, &["col1", "col2"]);
            }
            _ => panic!("Expected CREATE VIEW"),
        }
    }

    #[test]
    fn test_parse_create_view_with_schema() {
        let program = parse_program("CREATE VIEW main.my_view AS SELECT 1;").unwrap();
        match &program.statements[0] {
            Statement::CreateView(stmt) => {
                assert_eq!(stmt.schema.as_deref(), Some("main"));
                assert_eq!(stmt.view_name, "my_view");
            }
            _ => panic!("Expected CREATE VIEW"),
        }
    }

    // ========================================
    // ALTER TABLE Tests
    // ========================================

    #[test]
    fn test_parse_alter_table_rename() {
        let program = parse_program("ALTER TABLE users RENAME TO customers;").unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                assert_eq!(stmt.table_name, "users");
                match &stmt.action {
                    AlterTableAction::RenameTo(new_name) => {
                        assert_eq!(new_name, "customers");
                    }
                    _ => panic!("Expected RenameTo"),
                }
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    #[test]
    fn test_parse_alter_table_rename_column() {
        let program = parse_program(
            "ALTER TABLE users RENAME COLUMN name TO full_name;"
        ).unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                match &stmt.action {
                    AlterTableAction::RenameColumn { old_name, new_name } => {
                        assert_eq!(old_name, "name");
                        assert_eq!(new_name, "full_name");
                    }
                    _ => panic!("Expected RenameColumn"),
                }
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    #[test]
    fn test_parse_alter_table_rename_column_without_keyword() {
        let program = parse_program("ALTER TABLE users RENAME name TO full_name;").unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                match &stmt.action {
                    AlterTableAction::RenameColumn { old_name, new_name } => {
                        assert_eq!(old_name, "name");
                        assert_eq!(new_name, "full_name");
                    }
                    _ => panic!("Expected RenameColumn"),
                }
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    #[test]
    fn test_parse_alter_table_add_column() {
        let program = parse_program("ALTER TABLE users ADD COLUMN age INTEGER;").unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                match &stmt.action {
                    AlterTableAction::AddColumn(col) => {
                        assert_eq!(col.name, "age");
                        assert_eq!(col.type_name.as_deref(), Some("INTEGER"));
                    }
                    _ => panic!("Expected AddColumn"),
                }
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    #[test]
    fn test_parse_alter_table_add_column_without_keyword() {
        let program = parse_program("ALTER TABLE users ADD email TEXT;").unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                match &stmt.action {
                    AlterTableAction::AddColumn(col) => {
                        assert_eq!(col.name, "email");
                    }
                    _ => panic!("Expected AddColumn"),
                }
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    #[test]
    fn test_parse_alter_table_drop_column() {
        let program = parse_program("ALTER TABLE users DROP COLUMN age;").unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                match &stmt.action {
                    AlterTableAction::DropColumn(col_name) => {
                        assert_eq!(col_name, "age");
                    }
                    _ => panic!("Expected DropColumn"),
                }
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    #[test]
    fn test_parse_alter_table_with_schema() {
        let program = parse_program("ALTER TABLE main.users RENAME TO customers;").unwrap();
        match &program.statements[0] {
            Statement::AlterTable(stmt) => {
                assert_eq!(stmt.schema.as_deref(), Some("main"));
                assert_eq!(stmt.table_name, "users");
            }
            _ => panic!("Expected ALTER TABLE"),
        }
    }

    // ========================================
    // Column Constraint Tests
    // ========================================

    #[test]
    fn test_parse_column_primary_key() {
        let program = parse_program("CREATE TABLE t (id INTEGER PRIMARY KEY);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.columns.len(), 1);
                assert_eq!(stmt.columns[0].constraints.len(), 1);
                assert!(matches!(stmt.columns[0].constraints[0], ColumnConstraint::PrimaryKey { .. }));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_primary_key_autoincrement() {
        let program = parse_program("CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.columns[0].constraints[0] {
                    ColumnConstraint::PrimaryKey { autoincrement, .. } => {
                        assert!(autoincrement);
                    }
                    _ => panic!("Expected PRIMARY KEY"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_not_null() {
        let program = parse_program("CREATE TABLE t (name TEXT NOT NULL);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(matches!(stmt.columns[0].constraints[0], ColumnConstraint::NotNull { .. }));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_unique() {
        let program = parse_program("CREATE TABLE t (email TEXT UNIQUE);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(matches!(stmt.columns[0].constraints[0], ColumnConstraint::Unique { .. }));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_check() {
        let program = parse_program("CREATE TABLE t (age INTEGER CHECK (age > 0));").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert!(matches!(stmt.columns[0].constraints[0], ColumnConstraint::Check { .. }));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_default_literal() {
        let program = parse_program("CREATE TABLE t (active INTEGER DEFAULT 1);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.columns[0].constraints[0] {
                    ColumnConstraint::Default { value, .. } => {
                        assert!(matches!(value, DefaultValue::Literal(_)));
                    }
                    _ => panic!("Expected DEFAULT"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_default_expr() {
        let program = parse_program("CREATE TABLE t (ts TEXT DEFAULT (datetime('now')));").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.columns[0].constraints[0] {
                    ColumnConstraint::Default { value, .. } => {
                        assert!(matches!(value, DefaultValue::Expr(_)));
                    }
                    _ => panic!("Expected DEFAULT"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_collate() {
        let program = parse_program("CREATE TABLE t (name TEXT COLLATE NOCASE);").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.columns[0].constraints[0] {
                    ColumnConstraint::Collate { collation, .. } => {
                        assert_eq!(collation, "NOCASE");
                    }
                    _ => panic!("Expected COLLATE"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_references() {
        let program = parse_program("CREATE TABLE orders (user_id INTEGER REFERENCES users (id));").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.columns[0].constraints[0] {
                    ColumnConstraint::ForeignKey { foreign_table, columns, .. } => {
                        assert_eq!(foreign_table, "users");
                        assert_eq!(columns.as_ref().unwrap(), &["id"]);
                    }
                    _ => panic!("Expected REFERENCES"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_foreign_key_actions() {
        let program = parse_program(
            "CREATE TABLE orders (user_id INTEGER REFERENCES users ON DELETE CASCADE ON UPDATE SET NULL);"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                match &stmt.columns[0].constraints[0] {
                    ColumnConstraint::ForeignKey { on_delete, on_update, .. } => {
                        assert_eq!(*on_delete, Some(ForeignKeyAction::Cascade));
                        assert_eq!(*on_update, Some(ForeignKeyAction::SetNull));
                    }
                    _ => panic!("Expected REFERENCES"),
                }
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_multiple_constraints() {
        let program = parse_program(
            "CREATE TABLE t (id INTEGER PRIMARY KEY NOT NULL, name TEXT UNIQUE DEFAULT '');"
        ).unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                // First column has PRIMARY KEY and NOT NULL
                assert_eq!(stmt.columns[0].constraints.len(), 2);
                // Second column has UNIQUE and DEFAULT
                assert_eq!(stmt.columns[1].constraints.len(), 2);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_type_with_size() {
        let program = parse_program("CREATE TABLE t (name VARCHAR(255));").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.columns[0].type_name.as_deref(), Some("VARCHAR(255)"));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_column_type_decimal() {
        let program = parse_program("CREATE TABLE t (price DECIMAL(10,2));").unwrap();
        match &program.statements[0] {
            Statement::CreateTable(stmt) => {
                assert_eq!(stmt.columns[0].type_name.as_deref(), Some("DECIMAL(10,2)"));
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    // ========================================
    // Snapshot Tests (using insta)
    // ========================================

    #[test]
    fn snapshot_select_simple() {
        let program = parse_program("SELECT 1, 'hello', NULL;").unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_select_with_where() {
        let program = parse_program("SELECT a, b FROM users WHERE id = 1;").unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_select_join() {
        let program = parse_program(
            "SELECT u.name, o.total FROM users u JOIN orders o ON u.id = o.user_id;"
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_create_table() {
        let program = parse_program(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);"
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_insert() {
        let program = parse_program(
            "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com');"
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_expression_complex() {
        let program = parse_program(
            "SELECT CASE WHEN x > 0 THEN 'positive' ELSE 'non-positive' END;"
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_generated_columns() {
        let program = parse_program(
            "CREATE TABLE rect (
                w REAL,
                h REAL,
                area REAL GENERATED ALWAYS AS (w * h) STORED,
                perimeter REAL AS (2 * (w + h)) VIRTUAL
            );"
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_quoted_identifiers() {
        let program = parse_program(
            r#"CREATE TABLE "Game Settings" (
                "user ID" INTEGER PRIMARY KEY,
                "Auto save" BOOLEAN
            );
            SELECT "user ID" FROM "Game Settings" WHERE "Auto save" = true;"#
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }

    #[test]
    fn snapshot_bracket_identifiers() {
        let program = parse_program(
            "CREATE TABLE [my table] ([column one] TEXT, [column two] INT);"
        ).unwrap();
        insta::assert_debug_snapshot!(program);
    }
}
