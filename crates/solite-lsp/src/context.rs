//! Completion context detection for SQL statements.
//!
//! This module provides a state machine-based approach to detect the completion
//! context at any cursor position within a SQL statement. It works with raw tokens
//! so it handles incomplete and invalid SQL gracefully.

use std::collections::HashSet;
use solite_lexer::{lex, Token, TokenKind};

/// Check if a token kind is any type of identifier (plain, quoted, bracketed, backtick)
fn is_ident_token(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::QuotedIdent
            | TokenKind::BracketIdent
            | TokenKind::BacktickIdent
    )
}

/// Extract the name from an identifier token, handling dequoting
fn ident_name(sql: &str, token: &Token) -> String {
    let raw = &sql[token.span.start..token.span.end];
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

/// A reference to a table in the current query, possibly with an alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    /// The actual table name
    pub name: String,
    /// Optional alias for the table
    pub alias: Option<String>,
}

/// A reference to a CTE (Common Table Expression) in the current query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CteRef {
    /// The CTE name
    pub name: String,
    /// Column names (explicit or inferred from SELECT)
    pub columns: Vec<String>,
    /// Table names from unresolved SELECT * (for lazy schema resolution)
    pub star_sources: Vec<String>,
}

impl TableRef {
    pub fn new(name: String, alias: Option<String>) -> Self {
        Self { name, alias }
    }

    /// Returns the name to use for column qualification (alias if present, otherwise name).
    pub fn qualifier(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }

    /// Check if this table ref matches the given qualifier (alias or table name).
    /// Comparison is case-insensitive.
    pub fn matches_qualifier(&self, qualifier: &str) -> bool {
        let qualifier_lower = qualifier.to_lowercase();
        if let Some(ref alias) = self.alias {
            alias.to_lowercase() == qualifier_lower
        } else {
            self.name.to_lowercase() == qualifier_lower
        }
    }
}

/// The detected completion context at a cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    // ========================================
    // Table name contexts
    // ========================================
    /// Cursor is after FROM keyword - suggest table names
    AfterFrom {
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is after a table name in FROM clause - suggest JOIN keywords, WHERE, etc.
    AfterFromTable {
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is after JOIN keyword - suggest table names
    AfterJoin {
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is after a table name in JOIN clause - suggest ON, AS
    AfterJoinTable {
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is after INTO keyword (INSERT INTO) - suggest table names
    AfterInto,
    /// Cursor is after UPDATE keyword - suggest table names
    AfterUpdate,
    /// Cursor is after TABLE keyword (DROP TABLE, ALTER TABLE) - suggest table names
    AfterTable,
    /// Cursor is after INDEX keyword (DROP INDEX) - suggest index names
    AfterIndex,
    /// Cursor is after VIEW keyword (DROP VIEW) - suggest view names
    AfterView,
    /// Cursor is after ON keyword in CREATE INDEX - suggest table names
    AfterOn,

    // ========================================
    // Column name contexts
    // ========================================
    /// Cursor is in SELECT column list
    SelectColumns {
        /// Tables/aliases currently in scope
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is in INSERT column list: INSERT INTO t(|)
    InsertColumns {
        /// The target table name
        table_name: String,
    },
    /// Cursor is in UPDATE SET clause: UPDATE t SET |
    UpdateSet {
        /// The target table name
        table_name: String,
    },
    /// Cursor is in WHERE clause
    WhereClause {
        /// Tables/aliases in scope
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is in JOIN ON clause
    JoinOn {
        /// Tables from the left side of the join
        left_tables: Vec<TableRef>,
        /// The table on the right side of the join
        right_table: TableRef,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is in GROUP BY clause
    GroupByClause {
        /// Tables/aliases in scope
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is in HAVING clause
    HavingClause {
        /// Tables/aliases in scope
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },
    /// Cursor is in ORDER BY clause
    OrderByClause {
        /// Tables/aliases in scope
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },

    // ========================================
    // ALTER TABLE contexts
    // ========================================
    /// Cursor is after ALTER TABLE <name> - suggest ADD, DROP, RENAME keywords
    AlterTableAction {
        /// The table being altered
        table_name: String,
    },
    /// Cursor is after ALTER TABLE <name> DROP COLUMN - suggest column names
    AlterColumn {
        /// The table being altered
        table_name: String,
    },

    // ========================================
    // Keyword contexts
    // ========================================
    /// Cursor is after CREATE keyword - suggest TABLE, INDEX, VIEW, etc.
    AfterCreate,
    /// Cursor is after CREATE TABLE - suggest IF NOT EXISTS or table name
    AfterCreateTable,
    /// Cursor is after column type in CREATE TABLE - suggest column constraints
    CreateTableColumnConstraint,
    /// Cursor is after INSERT - suggest INTO, OR ABORT/FAIL/IGNORE/REPLACE/ROLLBACK
    AfterInsert,
    /// Cursor is after REPLACE - suggest INTO
    AfterReplace,
    /// Cursor is after DROP keyword - suggest TABLE, INDEX, VIEW, etc.
    AfterDrop,
    /// Cursor is after ALTER keyword - suggest TABLE
    AfterAlter,
    /// Cursor is at the start of a statement
    StatementStart {
        /// Prefix being typed (for filtering). None means cursor is at very start with nothing typed.
        prefix: Option<String>,
    },
    /// Cursor is in DELETE FROM ... WHERE clause
    DeleteWhere {
        /// The table being deleted from
        table_name: String,
    },

    /// Cursor is after an expression in WHERE clause - suggest AND, OR, ORDER BY, etc.
    AfterWhereExpr {
        /// Tables/aliases in scope
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },

    // ========================================
    // Expression contexts (reserved for Phase 7)
    // ========================================
    /// Cursor is in a general expression context (for function suggestions)
    #[allow(dead_code)]
    Expression {
        /// Tables/aliases in scope
        tables: Vec<TableRef>,
    },

    /// Cursor is in CREATE INDEX columns list: CREATE INDEX idx ON t(|)
    CreateIndexColumns {
        /// The table being indexed
        table_name: String,
    },

    /// Cursor is immediately after "qualifier." - suggest columns from that qualifier only
    /// This is used when the user types "alias." or "table." to qualify a column reference
    QualifiedColumn {
        /// The qualifier (alias or table name) before the dot
        qualifier: String,
        /// All tables/aliases in scope (to resolve the qualifier to a real table)
        tables: Vec<TableRef>,
        /// CTEs in scope for this query
        ctes: Vec<CteRef>,
    },

    /// No completion context detected
    None,
}

/// Internal state machine states for context detection.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ContextState {
    /// Initial state or after a statement ends
    Start,
    /// After WITH keyword (start of CTE)
    AfterWith,
    /// After CTE name, expecting ( for columns or AS
    AfterCteName,
    /// Inside CTE explicit column list: WITH foo(|)
    InCteColumns,
    /// After AS keyword in CTE
    AfterCteAs,
    /// After SELECT keyword
    AfterSelect,
    /// In the SELECT column list (before FROM)
    InSelectColumns,
    /// After FROM keyword
    AfterFrom,
    /// After a table name in FROM clause
    AfterFromTable,
    /// Expecting an alias after AS or table name
    ExpectAlias,
    /// After JOIN keyword
    AfterJoin,
    /// After a table name in JOIN clause
    AfterJoinTable,
    /// After ON keyword in JOIN
    AfterJoinOn,
    /// After WHERE keyword
    AfterWhere,
    /// In WHERE clause, after an expression (identifier, literal, etc.)
    InWhereExpr,
    /// After GROUP keyword
    AfterGroup,
    /// In GROUP BY clause
    InGroupBy,
    /// After HAVING keyword
    AfterHaving,
    /// After ORDER keyword
    AfterOrder,
    /// In ORDER BY clause
    InOrderBy,
    /// After INSERT keyword
    AfterInsert,
    /// After INSERT OR (waiting for conflict resolution keyword)
    AfterInsertOr,
    /// After REPLACE keyword
    AfterReplace,
    /// After INTO keyword
    AfterInto,
    /// After table name in INSERT statement
    AfterInsertTable,
    /// Inside INSERT column list parentheses
    InInsertColumns,
    /// After UPDATE keyword
    AfterUpdate,
    /// After table name in UPDATE statement
    AfterUpdateTable,
    /// After SET keyword
    AfterSet,
    /// After DELETE keyword
    AfterDelete,
    /// After DELETE FROM
    AfterDeleteFrom,
    /// After table name in DELETE statement
    AfterDeleteTable,
    /// After DELETE ... WHERE
    InDeleteWhere,
    /// After CREATE keyword
    AfterCreate,
    /// After CREATE TABLE (before table name, IF NOT EXISTS goes here)
    AfterCreateTable,
    /// After CREATE TABLE <name>
    AfterCreateTableName,
    /// Inside CREATE TABLE column definitions (start of column or after comma)
    InCreateTableColumns,
    /// After column name in CREATE TABLE
    AfterCreateTableColumnName,
    /// After column type in CREATE TABLE - suggest constraints
    AfterCreateTableColumnType,
    /// After DROP keyword
    AfterDrop,
    /// After ALTER keyword
    AfterAlter,
    /// After ALTER TABLE
    AfterAlterTable,
    /// After ALTER TABLE <name>
    AfterAlterTableName,
    /// After ALTER TABLE <name> DROP
    AfterAlterTableDrop,
    /// After ALTER TABLE <name> DROP COLUMN
    AfterAlterTableDropColumn,
    /// After CREATE INDEX
    AfterCreateIndex,
    /// After CREATE INDEX <name>
    AfterCreateIndexName,
    /// After CREATE INDEX <name> ON
    AfterCreateIndexOn,
    /// After CREATE INDEX <name> ON <table>
    AfterCreateIndexTable,
    /// Inside CREATE INDEX column list
    InCreateIndexColumns,
    /// After DROP TABLE
    AfterDropTable,
    /// After DROP INDEX
    AfterDropIndex,
    /// After DROP VIEW
    AfterDropView,
    // Note: InExpression state is reserved for future use
}

/// Detect the completion context at the given cursor offset.
///
/// This function tokenizes the source and runs a state machine to determine
/// what kind of completion should be offered at the cursor position.
pub fn detect_context(source: &str, cursor_offset: usize) -> CompletionContext {
    let tokens = lex(source);
    detect_context_from_tokens(&tokens, source, cursor_offset)
}

/// Check if the cursor is immediately after an "identifier." pattern.
/// Returns the identifier (qualifier) if found.
fn detect_qualifier_before_cursor(
    tokens: &[Token],
    source: &str,
    cursor_offset: usize,
) -> Option<String> {
    // Find the last two non-comment tokens before the cursor
    let relevant_tokens: Vec<_> = tokens
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Comment | TokenKind::BlockComment))
        .filter(|t| t.span.end <= cursor_offset)
        .collect();

    if relevant_tokens.len() < 2 {
        return None;
    }

    let last = relevant_tokens[relevant_tokens.len() - 1];
    let second_last = relevant_tokens[relevant_tokens.len() - 2];

    // Check if the pattern is: Ident Dot (and cursor is right after the dot)
    if last.kind == TokenKind::Dot
        && is_ident_token(&second_last.kind)
        && last.span.end == cursor_offset
    {
        let qualifier = ident_name(source, second_last);
        return Some(qualifier);
    }

    None
}

/// Collect all CTEs in scope at the cursor position.
/// This extracts CTE names and their explicit columns from WITH clauses.
fn collect_ctes_in_scope(tokens: &[Token], source: &str, cursor_offset: usize) -> Vec<CteRef> {
    let mut ctes: Vec<CteRef> = Vec::new();
    let mut current_cte_name: Option<String> = None;
    let mut current_cte_columns: Vec<String> = Vec::new();
    let mut has_explicit_columns = false;
    // Track * references to resolve from CTEs later
    let mut has_star = false;
    let mut star_sources: Vec<String> = Vec::new(); // CTE/table names to expand * from

    #[derive(Clone, PartialEq)]
    enum CteState {
        Start,
        AfterWith,
        AfterCteName,
        InCteColumns,
        AfterCteAs,
        // Inside CTE body: depth tracks parens, in_select_cols tracks if we're inferring columns
        InCteBody { depth: usize, in_select_cols: bool },
        // After seeing an identifier in SELECT columns, waiting for AS or comma/FROM
        AfterSelectIdent { depth: usize, last_ident: String },
        // After seeing AS in SELECT columns, waiting for alias
        AfterSelectAs { depth: usize },
        // After FROM in CTE body, looking for table/CTE name to resolve *
        AfterFrom { depth: usize },
    }

    /// Helper to resolve * columns from referenced CTEs
    fn resolve_star_columns(ctes: &[CteRef], star_sources: &[String]) -> Vec<String> {
        let mut cols = Vec::new();
        for source_name in star_sources {
            // Look up in already-collected CTEs (case-insensitive)
            if let Some(cte) = ctes.iter().find(|c| c.name.eq_ignore_ascii_case(source_name)) {
                cols.extend(cte.columns.iter().cloned());
            }
        }
        cols
    }

    let mut state = CteState::Start;

    for token in tokens {
        if token.span.start >= cursor_offset {
            break;
        }

        if matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment) {
            continue;
        }

        let token_text = || ident_name(source, token);

        // Reset on semicolon
        if token.kind == TokenKind::Semicolon {
            ctes.clear();
            current_cte_name = None;
            current_cte_columns.clear();
            has_explicit_columns = false;
            has_star = false;
            star_sources.clear();
            state = CteState::Start;
            continue;
        }

        state = match (&state, &token.kind) {
            (CteState::Start, TokenKind::With) => {
                ctes.clear();
                current_cte_name = None;
                current_cte_columns.clear();
                has_explicit_columns = false;
                has_star = false;
                star_sources.clear();
                CteState::AfterWith
            }
            (CteState::AfterWith, TokenKind::Recursive) => CteState::AfterWith,
            (CteState::AfterWith, kind) if is_ident_token(kind) => {
                current_cte_name = Some(token_text());
                current_cte_columns.clear();
                has_explicit_columns = false;
                has_star = false;
                star_sources.clear();
                CteState::AfterCteName
            }
            (CteState::AfterCteName, TokenKind::LParen) => {
                has_explicit_columns = true;
                CteState::InCteColumns
            }
            (CteState::AfterCteName, TokenKind::As) => CteState::AfterCteAs,
            (CteState::InCteColumns, kind) if is_ident_token(kind) => {
                current_cte_columns.push(token_text());
                CteState::InCteColumns
            }
            (CteState::InCteColumns, TokenKind::Comma) => CteState::InCteColumns,
            (CteState::InCteColumns, TokenKind::RParen) => CteState::AfterCteName,
            (CteState::AfterCteAs, TokenKind::LParen) => {
                CteState::InCteBody { depth: 1, in_select_cols: false }
            }
            // Inside CTE body - look for SELECT to start inferring columns
            (CteState::InCteBody { depth, .. }, TokenKind::Select) if !has_explicit_columns => {
                CteState::InCteBody { depth: *depth, in_select_cols: true }
            }
            // In SELECT columns - * means we need to resolve from FROM clause
            (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::Star) => {
                has_star = true;
                CteState::InCteBody { depth: *depth, in_select_cols: true }
            }
            // In SELECT columns - identifier starts a column expression
            (CteState::InCteBody { depth, in_select_cols: true }, kind) if is_ident_token(kind) => {
                CteState::AfterSelectIdent { depth: *depth, last_ident: token_text() }
            }
            // In SELECT columns - AS after a non-identifier expression (like `1 AS x`)
            (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::As) => {
                CteState::AfterSelectAs { depth: *depth }
            }
            // After identifier - AS means next identifier is the alias
            (CteState::AfterSelectIdent { depth, .. }, TokenKind::As) => {
                CteState::AfterSelectAs { depth: *depth }
            }
            // After identifier - comma means the identifier was the column name
            (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::Comma) => {
                current_cte_columns.push(last_ident.clone());
                CteState::InCteBody { depth: *depth, in_select_cols: true }
            }
            // After identifier - FROM means end of SELECT columns, start looking for table
            (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::From) => {
                current_cte_columns.push(last_ident.clone());
                CteState::AfterFrom { depth: *depth }
            }
            // In SELECT columns - FROM means end of columns, start looking for table
            (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::From) => {
                CteState::AfterFrom { depth: *depth }
            }
            // After identifier - WHERE/etc means end of SELECT columns
            (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::Where)
            | (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::Group)
            | (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::Order)
            | (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::Limit) => {
                current_cte_columns.push(last_ident.clone());
                CteState::InCteBody { depth: *depth, in_select_cols: false }
            }
            // After FROM - identifier is the table/CTE name for * resolution
            (CteState::AfterFrom { depth }, kind) if is_ident_token(kind) => {
                if has_star {
                    star_sources.push(token_text());
                }
                CteState::InCteBody { depth: *depth, in_select_cols: false }
            }
            // After AS - identifier is the alias
            (CteState::AfterSelectAs { depth }, kind) if is_ident_token(kind) => {
                current_cte_columns.push(token_text());
                CteState::InCteBody { depth: *depth, in_select_cols: true }
            }
            // After select ident - opening paren starts a function call, don't record this ident
            (CteState::AfterSelectIdent { depth, .. }, TokenKind::LParen) => {
                CteState::InCteBody { depth: depth + 1, in_select_cols: true }
            }
            // After AS - string literal is also valid (for literal aliases)
            (CteState::AfterSelectAs { depth }, TokenKind::String) => {
                current_cte_columns.push(token_text());
                CteState::InCteBody { depth: *depth, in_select_cols: true }
            }
            // Parenthesis tracking in CTE body
            (CteState::InCteBody { depth, in_select_cols }, TokenKind::LParen) => {
                CteState::InCteBody { depth: depth + 1, in_select_cols: *in_select_cols }
            }
            (CteState::InCteBody { depth, .. }, TokenKind::RParen)
            | (CteState::AfterFrom { depth }, TokenKind::RParen) => {
                if *depth == 1 {
                    // End of CTE body - resolve * and save the CTE
                    if has_star {
                        let star_cols = resolve_star_columns(&ctes, &star_sources);
                        // Prepend star columns (they come before explicit columns)
                        let mut all_cols = star_cols;
                        all_cols.append(&mut current_cte_columns);
                        current_cte_columns = all_cols;
                    }
                    if let Some(name) = current_cte_name.take() {
                        ctes.push(CteRef {
                            name,
                            columns: std::mem::take(&mut current_cte_columns),
                            star_sources: std::mem::take(&mut star_sources),
                        });
                    }
                    has_explicit_columns = false;
                    has_star = false;
                    CteState::AfterCteName
                } else {
                    CteState::InCteBody { depth: depth - 1, in_select_cols: false }
                }
            }
            // After identifier - RParen at depth 1 ends CTE body, save the last ident as column
            (CteState::AfterSelectIdent { depth, last_ident }, TokenKind::RParen) => {
                if *depth == 1 {
                    current_cte_columns.push(last_ident.clone());
                    // Resolve * columns from other CTEs
                    if has_star {
                        let star_cols = resolve_star_columns(&ctes, &star_sources);
                        let mut all_cols = star_cols;
                        all_cols.append(&mut current_cte_columns);
                        current_cte_columns = all_cols;
                    }
                    if let Some(name) = current_cte_name.take() {
                        ctes.push(CteRef {
                            name,
                            columns: std::mem::take(&mut current_cte_columns),
                            star_sources: std::mem::take(&mut star_sources),
                        });
                    }
                    has_explicit_columns = false;
                    has_star = false;
                    CteState::AfterCteName
                } else {
                    CteState::InCteBody { depth: depth - 1, in_select_cols: false }
                }
            }
            // WHERE/GROUP/ORDER/LIMIT end the SELECT column list (FROM is handled above)
            (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::Where)
            | (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::Group)
            | (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::Order)
            | (CteState::InCteBody { depth, in_select_cols: true }, TokenKind::Limit) => {
                CteState::InCteBody { depth: *depth, in_select_cols: false }
            }
            (CteState::InCteBody { depth, in_select_cols }, _) => {
                CteState::InCteBody { depth: *depth, in_select_cols: *in_select_cols }
            }
            (CteState::AfterCteName, TokenKind::Comma) => CteState::AfterWith,
            (CteState::AfterCteName, TokenKind::Select) => {
                // Main query started, CTEs are complete for this statement
                // Stay in Start state to detect semicolons that reset CTEs
                CteState::Start
            }
            (s, _) => s.clone(),
        };
    }

    ctes
}

/// Collect all tables in scope at the cursor position.
/// This runs a simplified version of the state machine just to extract tables.
fn collect_tables_in_scope(tokens: &[Token], source: &str, cursor_offset: usize) -> Vec<TableRef> {
    let mut tables_in_scope: Vec<TableRef> = Vec::new();
    let mut current_table_name: Option<String> = None;
    let mut join_right_table: Option<TableRef> = None;

    #[derive(Clone, PartialEq)]
    enum SimpleState {
        Start,
        AfterFrom,
        AfterFromTable,
        ExpectAlias,
        AfterJoin,
        AfterJoinTable,
        AfterJoinOn,
        Other,
    }

    let mut state = SimpleState::Start;

    for token in tokens {
        if token.span.start >= cursor_offset {
            break;
        }

        if matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment) {
            continue;
        }

        let token_text = || ident_name(source, token);

        // Reset on semicolon
        if token.kind == TokenKind::Semicolon {
            tables_in_scope.clear();
            current_table_name = None;
            join_right_table = None;
            state = SimpleState::Start;
            continue;
        }

        state = match (&state, &token.kind) {
            (_, TokenKind::From) => {
                // Could be SELECT FROM or DELETE FROM
                SimpleState::AfterFrom
            }
            (SimpleState::AfterFrom, kind) if is_ident_token(kind) => {
                current_table_name = Some(token_text());
                SimpleState::AfterFromTable
            }
            (SimpleState::AfterFromTable, TokenKind::As) => SimpleState::ExpectAlias,
            (SimpleState::AfterFromTable, kind) if is_ident_token(kind) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, Some(token_text())));
                }
                SimpleState::AfterFromTable
            }
            (SimpleState::ExpectAlias, kind) if is_ident_token(kind) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, Some(token_text())));
                }
                SimpleState::AfterFromTable
            }
            (SimpleState::AfterFromTable, TokenKind::Comma) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                SimpleState::AfterFrom
            }
            (SimpleState::AfterFromTable, TokenKind::Join | TokenKind::Inner | TokenKind::Left | TokenKind::Right | TokenKind::Full | TokenKind::Cross | TokenKind::Natural) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                SimpleState::AfterJoin
            }
            (SimpleState::AfterJoin, TokenKind::Join | TokenKind::Outer) => SimpleState::AfterJoin,
            (SimpleState::AfterJoin, kind) if is_ident_token(kind) => {
                join_right_table = Some(TableRef::new(token_text(), None));
                SimpleState::AfterJoinTable
            }
            (SimpleState::AfterJoinTable, TokenKind::As) => SimpleState::AfterJoinTable,
            (SimpleState::AfterJoinTable, kind) if is_ident_token(kind) => {
                if let Some(ref mut t) = join_right_table {
                    t.alias = Some(token_text());
                }
                SimpleState::AfterJoinTable
            }
            (SimpleState::AfterJoinTable, TokenKind::On) => SimpleState::AfterJoinOn,
            (SimpleState::AfterJoinOn, TokenKind::Join | TokenKind::Inner | TokenKind::Left | TokenKind::Right | TokenKind::Full | TokenKind::Cross | TokenKind::Natural) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                SimpleState::AfterJoin
            }
            (SimpleState::AfterJoinOn, TokenKind::Where | TokenKind::Group | TokenKind::Order) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                SimpleState::Other
            }
            (SimpleState::AfterFromTable, TokenKind::Where | TokenKind::Group | TokenKind::Order) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                SimpleState::Other
            }
            (SimpleState::AfterJoinTable, TokenKind::Where | TokenKind::Group | TokenKind::Order) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                SimpleState::Other
            }
            (s, _) => s.clone(),
        };
    }

    // Add any pending table
    if let Some(name) = current_table_name {
        tables_in_scope.push(TableRef::new(name, None));
    }
    if let Some(t) = join_right_table {
        tables_in_scope.push(t);
    }

    tables_in_scope
}

/// Look ahead from cursor position to find tables in FROM clause.
/// This is used for SELECT columns context where FROM comes after the cursor.
fn look_ahead_for_from_tables(tokens: &[Token], source: &str, cursor_offset: usize) -> Vec<TableRef> {
    let mut tables: Vec<TableRef> = Vec::new();
    let mut current_table_name: Option<String> = None;

    #[derive(Clone, PartialEq)]
    enum LookState {
        LookingForFrom,
        AfterFrom,
        AfterFromTable,
        ExpectAlias,
        AfterJoin,
        AfterJoinTable,
        Done,
    }

    let mut state = LookState::LookingForFrom;

    // Start from tokens after cursor
    for token in tokens {
        if token.span.start < cursor_offset {
            continue;
        }

        if matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment) {
            continue;
        }

        let token_text = || ident_name(source, token);

        // Stop on semicolon or another statement keyword
        if token.kind == TokenKind::Semicolon {
            break;
        }

        state = match (&state, &token.kind) {
            (LookState::LookingForFrom, TokenKind::From) => LookState::AfterFrom,
            (LookState::LookingForFrom, _) => LookState::LookingForFrom,

            (LookState::AfterFrom, kind) if is_ident_token(kind) => {
                current_table_name = Some(token_text());
                LookState::AfterFromTable
            }

            (LookState::AfterFromTable, TokenKind::As) => LookState::ExpectAlias,
            (LookState::AfterFromTable, kind) if is_ident_token(kind) => {
                // Implicit alias
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, Some(token_text())));
                }
                LookState::AfterFromTable
            }
            (LookState::ExpectAlias, kind) if is_ident_token(kind) => {
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, Some(token_text())));
                }
                LookState::AfterFromTable
            }
            (LookState::AfterFromTable, TokenKind::Comma) => {
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, None));
                }
                LookState::AfterFrom
            }

            // JOIN handling
            (LookState::AfterFromTable, TokenKind::Join | TokenKind::Inner | TokenKind::Left | TokenKind::Right | TokenKind::Full | TokenKind::Cross | TokenKind::Natural) => {
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, None));
                }
                LookState::AfterJoin
            }
            (LookState::AfterJoin, TokenKind::Join | TokenKind::Outer) => LookState::AfterJoin,
            (LookState::AfterJoin, kind) if is_ident_token(kind) => {
                current_table_name = Some(token_text());
                LookState::AfterJoinTable
            }
            (LookState::AfterJoinTable, TokenKind::As) => LookState::AfterJoinTable,
            (LookState::AfterJoinTable, kind) if is_ident_token(kind) => {
                // Alias for join table
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, Some(token_text())));
                }
                LookState::AfterJoinTable
            }
            (LookState::AfterJoinTable, TokenKind::On) => {
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, None));
                }
                LookState::AfterJoinTable
            }
            (LookState::AfterJoinTable, TokenKind::Join | TokenKind::Inner | TokenKind::Left | TokenKind::Right | TokenKind::Full | TokenKind::Cross | TokenKind::Natural) => {
                LookState::AfterJoin
            }

            // End of FROM clause
            (LookState::AfterFromTable | LookState::AfterJoinTable, TokenKind::Where | TokenKind::Group | TokenKind::Order | TokenKind::Limit | TokenKind::Union | TokenKind::Intersect | TokenKind::Except) => {
                if let Some(name) = current_table_name.take() {
                    tables.push(TableRef::new(name, None));
                }
                LookState::Done
            }

            (LookState::Done, _) => break,
            (s, _) => s.clone(),
        };
    }

    // Add any remaining table
    if let Some(name) = current_table_name {
        tables.push(TableRef::new(name, None));
    }

    tables
}

/// Detect completion context from pre-lexed tokens.
pub fn detect_context_from_tokens(
    tokens: &[Token],
    source: &str,
    cursor_offset: usize,
) -> CompletionContext {
    // First, collect CTEs in scope for this query
    let ctes_in_scope = collect_ctes_in_scope(tokens, source, cursor_offset);

    // Check if the cursor is immediately after "identifier." pattern
    // This is the qualified column completion case
    if let Some(qualifier) = detect_qualifier_before_cursor(tokens, source, cursor_offset) {
        // We need to run the state machine to get tables in scope
        let mut tables = collect_tables_in_scope(tokens, source, cursor_offset);
        // If no tables in scope yet (SELECT columns before FROM), look ahead
        if tables.is_empty() {
            tables = look_ahead_for_from_tables(tokens, source, cursor_offset);
        }
        return CompletionContext::QualifiedColumn { qualifier, tables, ctes: ctes_in_scope };
    }

    let mut state = ContextState::Start;
    let mut tables_in_scope: Vec<TableRef> = Vec::new();
    let mut current_table_name: Option<String> = None;
    let mut insert_table_name: Option<String> = None;
    let mut update_table_name: Option<String> = None;
    let mut delete_table_name: Option<String> = None;
    let mut alter_table_name: Option<String> = None;
    let mut index_table_name: Option<String> = None;
    let mut join_right_table: Option<TableRef> = None;
    let mut paren_depth: usize = 0;
    // CTE tracking (actual CTE state is handled by collect_ctes_in_scope)
    let mut ctes: Vec<CteRef> = Vec::new();
    let mut current_cte_name: Option<String> = None;
    let mut current_cte_columns: Vec<String> = Vec::new();
    // Track CTE body depth separately to allow normal SQL parsing inside
    let mut cte_body_depth: usize = 0;

    // Process tokens up to cursor position
    for token in tokens {
        // Stop if token starts at or after cursor
        if token.span.start >= cursor_offset {
            break;
        }

        // Skip comments
        if matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment) {
            continue;
        }

        // Get token text for identifiers (with dequoting)
        let token_text = || ident_name(source, token);

        // Track parenthesis depth (and CTE body depth)
        match token.kind {
            TokenKind::LParen => {
                paren_depth += 1;
                if cte_body_depth > 0 {
                    cte_body_depth += 1;
                }
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
                if cte_body_depth > 0 {
                    cte_body_depth -= 1;
                    if cte_body_depth == 0 {
                        // End of CTE body - save the CTE
                        if let Some(name) = current_cte_name.take() {
                            ctes.push(CteRef {
                                name,
                                columns: std::mem::take(&mut current_cte_columns),
                                star_sources: vec![],
                            });
                        }
                        // Reset for next CTE or main query
                        tables_in_scope.clear();
                        state = ContextState::AfterCteName;
                        continue; // Skip the state machine match for this token
                    }
                }
            }
            _ => {}
        }

        state = match (&state, &token.kind) {
            // ========================================
            // Statement end - MUST be first to override wildcards!
            // ========================================
            (_, TokenKind::Semicolon) => {
                tables_in_scope.clear();
                current_table_name = None;
                insert_table_name = None;
                update_table_name = None;
                delete_table_name = None;
                alter_table_name = None;
                index_table_name = None;
                join_right_table = None;
                paren_depth = 0;
                // Reset CTE tracking
                ctes.clear();
                current_cte_name = None;
                current_cte_columns.clear();
                cte_body_depth = 0;
                ContextState::Start
            }

            // ========================================
            // WITH clause (CTE) transitions
            // ========================================
            (ContextState::Start, TokenKind::With) => {
                ctes.clear();
                current_cte_name = None;
                current_cte_columns.clear();
                ContextState::AfterWith
            }
            (ContextState::AfterWith, TokenKind::Recursive) => {
                // WITH RECURSIVE - stay in AfterWith
                ContextState::AfterWith
            }
            (ContextState::AfterWith, kind) if is_ident_token(kind) => {
                current_cte_name = Some(token_text());
                current_cte_columns.clear();
                ContextState::AfterCteName
            }
            (ContextState::AfterCteName, TokenKind::LParen) => {
                // Start of explicit column list: WITH foo(
                ContextState::InCteColumns
            }
            (ContextState::AfterCteName, TokenKind::As) => {
                ContextState::AfterCteAs
            }
            (ContextState::InCteColumns, kind) if is_ident_token(kind) => {
                current_cte_columns.push(token_text());
                ContextState::InCteColumns
            }
            (ContextState::InCteColumns, TokenKind::Comma) => {
                ContextState::InCteColumns
            }
            (ContextState::InCteColumns, TokenKind::RParen) => {
                ContextState::AfterCteName
            }
            (ContextState::AfterCteAs, TokenKind::LParen) => {
                // Enter CTE body - track depth but allow normal SQL parsing inside
                cte_body_depth = 1;
                tables_in_scope.clear();
                ContextState::Start // Start fresh for parsing SELECT inside CTE body
            }
            // After CTE body, comma means another CTE
            (ContextState::AfterCteName, TokenKind::Comma) => {
                ContextState::AfterWith
            }
            // After CTE body, SELECT means main query
            (ContextState::AfterCteName, TokenKind::Select) => {
                tables_in_scope.clear();
                ContextState::AfterSelect
            }

            // ========================================
            // Statement start transitions
            // ========================================
            (ContextState::Start, TokenKind::Select) => {
                tables_in_scope.clear();
                ContextState::AfterSelect
            }
            (ContextState::Start, TokenKind::Insert) => {
                insert_table_name = None;
                ContextState::AfterInsert
            }
            (ContextState::Start, TokenKind::Replace) => {
                insert_table_name = None;
                ContextState::AfterReplace
            }
            (ContextState::Start, TokenKind::Update) => {
                update_table_name = None;
                ContextState::AfterUpdate
            }
            (ContextState::Start, TokenKind::Delete) => {
                delete_table_name = None;
                ContextState::AfterDelete
            }
            (ContextState::Start, TokenKind::Create) => ContextState::AfterCreate,
            (ContextState::Start, TokenKind::Drop) => ContextState::AfterDrop,
            (ContextState::Start, TokenKind::Alter) => ContextState::AfterAlter,

            // ========================================
            // SELECT statement transitions
            // ========================================
            (ContextState::AfterSelect, TokenKind::Distinct | TokenKind::All) => {
                ContextState::AfterSelect
            }
            (ContextState::AfterSelect, _) if !matches!(token.kind, TokenKind::From) => {
                ContextState::InSelectColumns
            }
            (ContextState::InSelectColumns, TokenKind::From) => ContextState::AfterFrom,
            (ContextState::InSelectColumns, _) => ContextState::InSelectColumns,
            (ContextState::AfterSelect, TokenKind::From) => ContextState::AfterFrom,

            // FROM clause
            (ContextState::AfterFrom, kind) if is_ident_token(kind) => {
                current_table_name = Some(token_text());
                ContextState::AfterFromTable
            }
            (ContextState::AfterFromTable, TokenKind::As) => ContextState::ExpectAlias,
            (ContextState::AfterFromTable, kind) if is_ident_token(kind) => {
                // Implicit alias
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, Some(token_text())));
                }
                ContextState::AfterFromTable
            }
            (ContextState::ExpectAlias, kind) if is_ident_token(kind) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, Some(token_text())));
                }
                ContextState::AfterFromTable
            }
            (ContextState::AfterFromTable, TokenKind::Comma) => {
                // Multiple tables in FROM
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                ContextState::AfterFrom
            }

            // JOIN
            (
                ContextState::AfterFromTable,
                TokenKind::Join
                | TokenKind::Inner
                | TokenKind::Left
                | TokenKind::Right
                | TokenKind::Full
                | TokenKind::Cross
                | TokenKind::Natural,
            ) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                ContextState::AfterJoin
            }
            (ContextState::AfterJoin, TokenKind::Join) => ContextState::AfterJoin,
            (ContextState::AfterJoin, TokenKind::Outer) => ContextState::AfterJoin,
            (ContextState::AfterJoin, kind) if is_ident_token(kind) => {
                join_right_table = Some(TableRef::new(token_text(), None));
                ContextState::AfterJoinTable
            }
            (ContextState::AfterJoinTable, TokenKind::As) => {
                // Keep state, next ident is alias
                ContextState::AfterJoinTable
            }
            (ContextState::AfterJoinTable, kind) if is_ident_token(kind) => {
                // Alias for join table
                if let Some(ref mut t) = join_right_table {
                    t.alias = Some(token_text());
                }
                ContextState::AfterJoinTable
            }
            (ContextState::AfterJoinTable, TokenKind::On) => ContextState::AfterJoinOn,
            (ContextState::AfterJoinOn, TokenKind::Join | TokenKind::Inner | TokenKind::Left | TokenKind::Right | TokenKind::Full | TokenKind::Cross | TokenKind::Natural) => {
                // Another join - add the right table to scope
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                ContextState::AfterJoin
            }
            (ContextState::AfterJoinOn, TokenKind::Where) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                ContextState::AfterWhere
            }
            (ContextState::AfterJoinOn, TokenKind::Group) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                ContextState::AfterGroup
            }
            (ContextState::AfterJoinOn, TokenKind::Order) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                ContextState::AfterOrder
            }
            (ContextState::AfterJoinOn, _) => ContextState::AfterJoinOn,

            // WHERE clause
            (ContextState::AfterFromTable, TokenKind::Where) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                ContextState::AfterWhere
            }
            (ContextState::AfterJoinTable, TokenKind::Where) => {
                if let Some(t) = join_right_table.take() {
                    tables_in_scope.push(t);
                }
                ContextState::AfterWhere
            }
            (ContextState::AfterWhere, TokenKind::Group) => ContextState::AfterGroup,
            (ContextState::AfterWhere, TokenKind::Order) => ContextState::AfterOrder,
            // After identifier/literal in WHERE, transition to InWhereExpr for operator suggestions
            (ContextState::AfterWhere, kind) if is_ident_token(kind) => ContextState::InWhereExpr,
            (ContextState::AfterWhere, TokenKind::Integer | TokenKind::Float | TokenKind::String | TokenKind::Null | TokenKind::True | TokenKind::False) => {
                ContextState::InWhereExpr
            }
            (ContextState::AfterWhere, _) => ContextState::AfterWhere,
            // In WHERE after an expression - AND/OR go back to AfterWhere
            (ContextState::InWhereExpr, TokenKind::And | TokenKind::Or) => ContextState::AfterWhere,
            // Comparison operators - stay in expression mode, next will be value
            (ContextState::InWhereExpr, TokenKind::Eq | TokenKind::Ne | TokenKind::BangEq | TokenKind::Lt | TokenKind::Le | TokenKind::Gt | TokenKind::Ge) => {
                ContextState::AfterWhere
            }
            // LIKE, BETWEEN, IN, IS operators
            (ContextState::InWhereExpr, TokenKind::Like | TokenKind::Between | TokenKind::In | TokenKind::Is) => {
                ContextState::AfterWhere
            }
            // Clause keywords
            (ContextState::InWhereExpr, TokenKind::Group) => ContextState::AfterGroup,
            (ContextState::InWhereExpr, TokenKind::Order) => ContextState::AfterOrder,
            // Another identifier (e.g., function call, qualified column)
            (ContextState::InWhereExpr, kind) if is_ident_token(kind) => ContextState::InWhereExpr,
            (ContextState::InWhereExpr, _) => ContextState::InWhereExpr,

            // GROUP BY clause
            (ContextState::AfterFromTable, TokenKind::Group) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                ContextState::AfterGroup
            }
            (ContextState::AfterGroup, TokenKind::By) => ContextState::InGroupBy,
            (ContextState::InGroupBy, TokenKind::Having) => ContextState::AfterHaving,
            (ContextState::InGroupBy, TokenKind::Order) => ContextState::AfterOrder,
            (ContextState::InGroupBy, _) => ContextState::InGroupBy,

            // HAVING clause
            (ContextState::AfterHaving, TokenKind::Order) => ContextState::AfterOrder,
            (ContextState::AfterHaving, _) => ContextState::AfterHaving,

            // ORDER BY clause
            (ContextState::AfterFromTable, TokenKind::Order) => {
                if let Some(name) = current_table_name.take() {
                    tables_in_scope.push(TableRef::new(name, None));
                }
                ContextState::AfterOrder
            }
            (ContextState::AfterOrder, TokenKind::By) => ContextState::InOrderBy,
            (ContextState::InOrderBy, _) => ContextState::InOrderBy,

            // ========================================
            // INSERT statement transitions
            // ========================================
            (ContextState::AfterInsert, TokenKind::Into) => ContextState::AfterInto,
            (ContextState::AfterInsert, TokenKind::Or) => ContextState::AfterInsertOr,
            (ContextState::AfterInsertOr, TokenKind::Abort | TokenKind::Fail | TokenKind::Ignore | TokenKind::Replace | TokenKind::Rollback) => {
                // After conflict resolution, still need INTO
                ContextState::AfterInsert
            }
            // REPLACE INTO is shorthand for INSERT OR REPLACE INTO
            (ContextState::AfterReplace, TokenKind::Into) => ContextState::AfterInto,
            (ContextState::AfterInto, kind) if is_ident_token(kind) => {
                insert_table_name = Some(token_text());
                ContextState::AfterInsertTable
            }
            (ContextState::AfterInsertTable, TokenKind::LParen) => {
                ContextState::InInsertColumns
            }
            (ContextState::InInsertColumns, TokenKind::RParen) => {
                ContextState::AfterInsertTable
            }
            (ContextState::InInsertColumns, _) => ContextState::InInsertColumns,

            // ========================================
            // UPDATE statement transitions
            // ========================================
            (ContextState::AfterUpdate, kind) if is_ident_token(kind) => {
                update_table_name = Some(token_text());
                ContextState::AfterUpdateTable
            }
            (ContextState::AfterUpdateTable, TokenKind::Set) => ContextState::AfterSet,
            (ContextState::AfterSet, TokenKind::Where) => {
                // UPDATE ... SET ... WHERE - transition to expression/where context
                tables_in_scope.clear();
                if let Some(ref name) = update_table_name {
                    tables_in_scope.push(TableRef::new(name.clone(), None));
                }
                ContextState::AfterWhere
            }
            (ContextState::AfterSet, _) => ContextState::AfterSet,

            // ========================================
            // DELETE statement transitions
            // ========================================
            (ContextState::AfterDelete, TokenKind::From) => ContextState::AfterDeleteFrom,
            (ContextState::AfterDeleteFrom, kind) if is_ident_token(kind) => {
                delete_table_name = Some(token_text());
                ContextState::AfterDeleteTable
            }
            (ContextState::AfterDeleteTable, TokenKind::Where) => ContextState::InDeleteWhere,
            (ContextState::InDeleteWhere, _) => ContextState::InDeleteWhere,

            // ========================================
            // CREATE statement transitions
            // ========================================
            (ContextState::AfterCreate, TokenKind::Table) => ContextState::AfterCreateTable,
            // CREATE TABLE IF NOT EXISTS stays in AfterCreateTable
            (ContextState::AfterCreateTable, TokenKind::If) => ContextState::AfterCreateTable,
            (ContextState::AfterCreateTable, TokenKind::Not) => ContextState::AfterCreateTable,
            (ContextState::AfterCreateTable, TokenKind::Exists) => ContextState::AfterCreateTable,
            // CREATE TABLE <name> transitions to AfterCreateTableName
            (ContextState::AfterCreateTable, kind) if is_ident_token(kind) => {
                ContextState::AfterCreateTableName
            }
            // CREATE TABLE <name> ( transitions to InCreateTableColumns
            (ContextState::AfterCreateTableName, TokenKind::LParen) => {
                ContextState::InCreateTableColumns
            }
            // Inside column definitions, ) ends the columns
            (ContextState::InCreateTableColumns, TokenKind::RParen) => ContextState::Start,
            // Column name starts a new column definition
            (ContextState::InCreateTableColumns, kind) if is_ident_token(kind) => {
                ContextState::AfterCreateTableColumnName
            }
            // After column name, an identifier is the type
            (ContextState::AfterCreateTableColumnName, kind) if is_ident_token(kind) => {
                ContextState::AfterCreateTableColumnType
            }
            // After column name, ( could be for type params or constraints - stay in column name state
            (ContextState::AfterCreateTableColumnName, TokenKind::LParen) => {
                ContextState::AfterCreateTableColumnName
            }
            // After column type, comma starts a new column
            (ContextState::AfterCreateTableColumnType, TokenKind::Comma) => {
                ContextState::InCreateTableColumns
            }
            // After column type, ) ends the table definition
            (ContextState::AfterCreateTableColumnType, TokenKind::RParen) => ContextState::Start,
            // After column type, ( is for type parameters like VARCHAR(255)
            (ContextState::AfterCreateTableColumnType, TokenKind::LParen) => {
                ContextState::AfterCreateTableColumnType
            }
            // After column type, constraint keywords keep us in column type state
            (ContextState::AfterCreateTableColumnType, TokenKind::Primary | TokenKind::Not |
             TokenKind::Unique | TokenKind::Check | TokenKind::Default | TokenKind::Collate |
             TokenKind::References | TokenKind::Generated | TokenKind::Constraint |
             TokenKind::Null | TokenKind::Key | TokenKind::As | TokenKind::Always) => {
                ContextState::AfterCreateTableColumnType
            }
            // After column type, identifiers could be part of constraints (e.g., collation name)
            (ContextState::AfterCreateTableColumnType, kind) if is_ident_token(kind) => {
                ContextState::AfterCreateTableColumnType
            }
            // Numbers and other literals in constraints
            (ContextState::AfterCreateTableColumnType, TokenKind::Integer | TokenKind::Float |
             TokenKind::String | TokenKind::Blob) => {
                ContextState::AfterCreateTableColumnType
            }

            (ContextState::AfterCreate, TokenKind::Index | TokenKind::Unique) => {
                ContextState::AfterCreateIndex
            }
            (ContextState::AfterCreateIndex, TokenKind::Index) => ContextState::AfterCreateIndex,
            (ContextState::AfterCreateIndex, TokenKind::If) => ContextState::AfterCreateIndex,
            (ContextState::AfterCreateIndex, TokenKind::Not) => ContextState::AfterCreateIndex,
            (ContextState::AfterCreateIndex, TokenKind::Exists) => ContextState::AfterCreateIndex,
            (ContextState::AfterCreateIndex, kind) if is_ident_token(kind) => {
                ContextState::AfterCreateIndexName
            }
            (ContextState::AfterCreateIndexName, TokenKind::On) => ContextState::AfterCreateIndexOn,
            (ContextState::AfterCreateIndexOn, kind) if is_ident_token(kind) => {
                index_table_name = Some(token_text());
                ContextState::AfterCreateIndexTable
            }
            (ContextState::AfterCreateIndexTable, TokenKind::LParen) => {
                ContextState::InCreateIndexColumns
            }
            (ContextState::InCreateIndexColumns, TokenKind::RParen) => {
                ContextState::Start
            }
            (ContextState::InCreateIndexColumns, _) => ContextState::InCreateIndexColumns,

            // ========================================
            // DROP statement transitions
            // ========================================
            (ContextState::AfterDrop, TokenKind::Table) => ContextState::AfterDropTable,
            (ContextState::AfterDrop, TokenKind::Index) => ContextState::AfterDropIndex,
            (ContextState::AfterDrop, TokenKind::View) => ContextState::AfterDropView,
            (ContextState::AfterDropTable, TokenKind::If) => ContextState::AfterDropTable,
            (ContextState::AfterDropTable, TokenKind::Exists) => ContextState::AfterDropTable,
            (ContextState::AfterDropIndex, TokenKind::If) => ContextState::AfterDropIndex,
            (ContextState::AfterDropIndex, TokenKind::Exists) => ContextState::AfterDropIndex,
            (ContextState::AfterDropView, TokenKind::If) => ContextState::AfterDropView,
            (ContextState::AfterDropView, TokenKind::Exists) => ContextState::AfterDropView,

            // ========================================
            // ALTER statement transitions
            // ========================================
            (ContextState::AfterAlter, TokenKind::Table) => ContextState::AfterAlterTable,
            (ContextState::AfterAlterTable, kind) if is_ident_token(kind) => {
                alter_table_name = Some(token_text());
                ContextState::AfterAlterTableName
            }
            (ContextState::AfterAlterTableName, TokenKind::Drop) => {
                ContextState::AfterAlterTableDrop
            }
            (ContextState::AfterAlterTableDrop, TokenKind::Column) => {
                ContextState::AfterAlterTableDropColumn
            }
            (ContextState::AfterAlterTableName, _) => ContextState::AfterAlterTableName,

            // Default: keep current state
            (state, _) => state.clone(),
        };
    }

    // For SELECT columns context, if no tables are in scope yet, look ahead for FROM clause
    let final_tables = if matches!(state, ContextState::AfterSelect | ContextState::InSelectColumns) && tables_in_scope.is_empty() {
        look_ahead_for_from_tables(tokens, source, cursor_offset)
    } else {
        tables_in_scope
    };

    // Detect prefix for statement start context
    // Look for a partial identifier token that ends at or contains the cursor
    let prefix = if matches!(state, ContextState::Start) {
        tokens
            .iter()
            .filter(|t| !matches!(t.kind, TokenKind::Comment | TokenKind::BlockComment))
            .find(|t| {
                // Token ends at cursor (user just finished typing) or contains cursor
                (t.span.end == cursor_offset || (t.span.start < cursor_offset && t.span.end > cursor_offset))
                    && is_ident_token(&t.kind)
            })
            .map(|t| {
                // Extract the portion of the token up to the cursor
                let end = cursor_offset.min(t.span.end);
                source[t.span.start..end].to_lowercase()
            })
    } else {
        None
    };

    // Convert final state to CompletionContext
    state_to_context(
        state,
        final_tables,
        current_table_name,
        insert_table_name,
        update_table_name,
        delete_table_name,
        alter_table_name,
        index_table_name,
        join_right_table,
        ctes_in_scope,
        prefix,
    )
}

#[allow(clippy::too_many_arguments)]
fn state_to_context(
    state: ContextState,
    tables_in_scope: Vec<TableRef>,
    _current_table_name: Option<String>,
    insert_table_name: Option<String>,
    update_table_name: Option<String>,
    delete_table_name: Option<String>,
    alter_table_name: Option<String>,
    index_table_name: Option<String>,
    join_right_table: Option<TableRef>,
    ctes_in_scope: Vec<CteRef>,
    prefix: Option<String>,
) -> CompletionContext {
    match state {
        ContextState::Start => CompletionContext::StatementStart { prefix },

        // CTE contexts - treat as statement start or suggest CTE names
        ContextState::AfterWith | ContextState::AfterCteName
        | ContextState::InCteColumns | ContextState::AfterCteAs => CompletionContext::None,

        // SELECT contexts
        ContextState::AfterSelect | ContextState::InSelectColumns => {
            CompletionContext::SelectColumns {
                tables: tables_in_scope,
                ctes: ctes_in_scope,
            }
        }
        ContextState::AfterFrom => CompletionContext::AfterFrom {
            ctes: ctes_in_scope,
        },
        ContextState::AfterFromTable | ContextState::ExpectAlias => {
            // After a table name, suggest JOIN keywords, WHERE, etc.
            CompletionContext::AfterFromTable {
                ctes: ctes_in_scope,
            }
        }
        ContextState::AfterJoin => CompletionContext::AfterJoin {
            ctes: ctes_in_scope,
        },
        ContextState::AfterJoinTable => CompletionContext::AfterJoinTable {
            ctes: ctes_in_scope,
        },
        ContextState::AfterJoinOn => {
            let left_tables = tables_in_scope;
            let right_table = join_right_table.unwrap_or_else(|| TableRef::new("".to_string(), None));
            CompletionContext::JoinOn {
                left_tables,
                right_table,
                ctes: ctes_in_scope,
            }
        }
        ContextState::AfterWhere => CompletionContext::WhereClause {
            tables: tables_in_scope,
            ctes: ctes_in_scope,
        },
        ContextState::InWhereExpr => CompletionContext::AfterWhereExpr {
            tables: tables_in_scope,
            ctes: ctes_in_scope,
        },
        ContextState::AfterGroup | ContextState::InGroupBy => {
            CompletionContext::GroupByClause {
                tables: tables_in_scope,
                ctes: ctes_in_scope,
            }
        }
        ContextState::AfterHaving => CompletionContext::HavingClause {
            tables: tables_in_scope,
            ctes: ctes_in_scope,
        },
        ContextState::AfterOrder | ContextState::InOrderBy => {
            CompletionContext::OrderByClause {
                tables: tables_in_scope,
                ctes: ctes_in_scope,
            }
        }

        // INSERT contexts
        ContextState::AfterInsert => CompletionContext::AfterInsert,
        ContextState::AfterInsertOr => CompletionContext::None, // waiting for conflict keyword
        ContextState::AfterReplace => CompletionContext::AfterReplace,
        ContextState::AfterInto => CompletionContext::AfterInto,
        ContextState::AfterInsertTable => {
            if let Some(name) = insert_table_name {
                CompletionContext::InsertColumns { table_name: name }
            } else {
                CompletionContext::None
            }
        }
        ContextState::InInsertColumns => {
            if let Some(name) = insert_table_name {
                CompletionContext::InsertColumns { table_name: name }
            } else {
                CompletionContext::None
            }
        }

        // UPDATE contexts
        ContextState::AfterUpdate => CompletionContext::AfterUpdate,
        ContextState::AfterUpdateTable => {
            if let Some(name) = update_table_name {
                CompletionContext::UpdateSet { table_name: name }
            } else {
                CompletionContext::None
            }
        }
        ContextState::AfterSet => {
            if let Some(name) = update_table_name {
                CompletionContext::UpdateSet { table_name: name }
            } else {
                CompletionContext::None
            }
        }

        // DELETE contexts
        ContextState::AfterDelete => CompletionContext::StatementStart { prefix: None },
        ContextState::AfterDeleteFrom => CompletionContext::AfterFrom {
            ctes: vec![], // DELETE doesn't have CTEs
        },
        ContextState::AfterDeleteTable => CompletionContext::None,
        ContextState::InDeleteWhere => {
            if let Some(name) = delete_table_name {
                CompletionContext::DeleteWhere { table_name: name }
            } else {
                CompletionContext::None
            }
        }

        // CREATE contexts
        ContextState::AfterCreate => CompletionContext::AfterCreate,
        ContextState::AfterCreateTable => CompletionContext::AfterCreateTable,
        // After table name or at start of column definitions - no specific completions
        ContextState::AfterCreateTableName
        | ContextState::InCreateTableColumns
        | ContextState::AfterCreateTableColumnName => CompletionContext::None,
        // After column type - suggest constraints
        ContextState::AfterCreateTableColumnType => CompletionContext::CreateTableColumnConstraint,
        ContextState::AfterCreateIndex
        | ContextState::AfterCreateIndexName => CompletionContext::None,
        ContextState::AfterCreateIndexOn => CompletionContext::AfterOn,
        ContextState::AfterCreateIndexTable => {
            if let Some(name) = index_table_name {
                CompletionContext::CreateIndexColumns { table_name: name }
            } else {
                CompletionContext::None
            }
        }
        ContextState::InCreateIndexColumns => {
            if let Some(name) = index_table_name {
                CompletionContext::CreateIndexColumns { table_name: name }
            } else {
                CompletionContext::None
            }
        }

        // DROP contexts
        ContextState::AfterDrop => CompletionContext::AfterDrop,
        ContextState::AfterDropTable => CompletionContext::AfterTable,
        ContextState::AfterDropIndex => CompletionContext::AfterIndex,
        ContextState::AfterDropView => CompletionContext::AfterView,

        // ALTER contexts
        ContextState::AfterAlter => CompletionContext::AfterAlter,
        ContextState::AfterAlterTable => CompletionContext::AfterTable,
        ContextState::AfterAlterTableName => {
            if let Some(name) = alter_table_name {
                CompletionContext::AlterTableAction { table_name: name }
            } else {
                CompletionContext::None
            }
        }
        ContextState::AfterAlterTableDrop => {
            if let Some(name) = alter_table_name {
                CompletionContext::AlterTableAction { table_name: name }
            } else {
                CompletionContext::None
            }
        }
        ContextState::AfterAlterTableDropColumn => {
            if let Some(name) = alter_table_name {
                CompletionContext::AlterColumn { table_name: name }
            } else {
                CompletionContext::None
            }
        }
    }
}

/// Extract column names that have already been listed in an INSERT INTO statement.
///
/// For example, in "INSERT INTO users (id, name, " this would return {"id", "name"}.
/// Column names are normalized to lowercase for case-insensitive comparison.
pub fn extract_used_insert_columns(source: &str, cursor_offset: usize) -> HashSet<String> {
    let tokens = lex(source);
    let mut used_columns = HashSet::new();
    let mut in_insert = false;
    let mut after_table = false;
    let mut in_columns = false;
    let mut paren_depth = 0;

    for token in tokens {
        // Stop if token starts at or after cursor
        if token.span.start >= cursor_offset {
            break;
        }

        // Skip comments
        if matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment) {
            continue;
        }

        // Reset state on semicolon
        if token.kind == TokenKind::Semicolon {
            in_insert = false;
            after_table = false;
            in_columns = false;
            paren_depth = 0;
            used_columns.clear();
            continue;
        }

        // Track INSERT state
        if token.kind == TokenKind::Insert {
            in_insert = true;
            after_table = false;
            in_columns = false;
            paren_depth = 0;
            used_columns.clear();
            continue;
        }

        if in_insert && token.kind == TokenKind::Into {
            continue;
        }

        // After INTO, the next ident is the table name
        if in_insert && !after_table && is_ident_token(&token.kind) {
            after_table = true;
            continue;
        }

        // After table name, look for opening paren
        if in_insert && after_table && !in_columns {
            if token.kind == TokenKind::LParen {
                in_columns = true;
                paren_depth = 1;
            }
            continue;
        }

        // Inside the column list
        if in_columns {
            match token.kind {
                TokenKind::LParen => {
                    paren_depth += 1;
                }
                TokenKind::RParen => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        in_columns = false;
                    }
                }
                _ if is_ident_token(&token.kind) && paren_depth == 1 => {
                    // This is a column name at the top level of the column list
                    let col_name = ident_name(source, &token).to_lowercase();
                    used_columns.insert(col_name);
                }
                _ => {}
            }
        }
    }

    used_columns
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to get context at the end of a string
    fn context_at_end(sql: &str) -> CompletionContext {
        detect_context(sql, sql.len())
    }

    // ========================================
    // Statement start tests
    // ========================================

    #[test]
    fn test_empty_string() {
        assert_eq!(context_at_end(""), CompletionContext::StatementStart { prefix: None });
    }

    #[test]
    fn test_after_semicolon() {
        assert_eq!(
            context_at_end("SELECT 1;"),
            CompletionContext::StatementStart { prefix: None }
        );
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(context_at_end("   "), CompletionContext::StatementStart { prefix: None });
    }

    #[test]
    fn test_partial_keyword() {
        // Typing "s" should give prefix "s"
        assert_eq!(context_at_end("s"), CompletionContext::StatementStart { prefix: Some("s".to_string()) });
        // Typing "sel" should give prefix "sel"
        assert_eq!(context_at_end("sel"), CompletionContext::StatementStart { prefix: Some("sel".to_string()) });
    }

    // ========================================
    // SELECT tests
    // ========================================

    #[test]
    fn test_after_select() {
        let ctx = context_at_end("SELECT ");
        assert!(matches!(ctx, CompletionContext::SelectColumns { .. }));
    }

    #[test]
    fn test_select_columns_comma() {
        let ctx = context_at_end("SELECT a, ");
        assert!(matches!(ctx, CompletionContext::SelectColumns { .. }));
    }

    #[test]
    fn test_after_from() {
        assert!(matches!(context_at_end("SELECT * FROM "), CompletionContext::AfterFrom { .. }));
    }

    #[test]
    fn test_after_from_table() {
        let ctx = context_at_end("SELECT * FROM users ");
        // After table name, in AfterFromTable context for JOINs, WHERE, etc.
        assert!(matches!(ctx, CompletionContext::AfterFromTable { .. }));
    }

    #[test]
    fn test_select_with_table_alias() {
        // Cursor at position 9 (after "u.")
        // With the new QualifiedColumn detection, this should return QualifiedColumn
        // Look-ahead finds the FROM clause even though cursor is before it
        let ctx = detect_context("SELECT u. FROM users AS u", 9);
        if let CompletionContext::QualifiedColumn { qualifier, tables, .. } = ctx {
            assert_eq!(qualifier, "u");
            // Look-ahead finds tables from FROM clause
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].name, "users");
            assert_eq!(tables[0].alias, Some("u".to_string()));
        } else {
            panic!("Expected QualifiedColumn context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_where_clause() {
        let ctx = context_at_end("SELECT * FROM users WHERE ");
        if let CompletionContext::WhereClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].name, "users");
        } else {
            panic!("Expected WhereClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_where_with_alias() {
        let ctx = context_at_end("SELECT * FROM users u WHERE ");
        if let CompletionContext::WhereClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].name, "users");
            assert_eq!(tables[0].alias, Some("u".to_string()));
        } else {
            panic!("Expected WhereClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_where_with_as_alias() {
        let ctx = context_at_end("SELECT * FROM users AS u WHERE ");
        if let CompletionContext::WhereClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].name, "users");
            assert_eq!(tables[0].alias, Some("u".to_string()));
        } else {
            panic!("Expected WhereClause context, got {:?}", ctx);
        }
    }

    // ========================================
    // JOIN tests
    // ========================================

    #[test]
    fn test_after_join() {
        let ctx = context_at_end("SELECT * FROM users JOIN ");
        assert!(matches!(ctx, CompletionContext::AfterJoin { .. }));
    }

    #[test]
    fn test_after_left_join() {
        let ctx = context_at_end("SELECT * FROM users LEFT JOIN ");
        assert!(matches!(ctx, CompletionContext::AfterJoin { .. }));
    }

    #[test]
    fn test_join_on() {
        let ctx = context_at_end("SELECT * FROM users u JOIN orders o ON ");
        if let CompletionContext::JoinOn {
            left_tables,
            right_table,
            ..
        } = ctx
        {
            assert_eq!(left_tables.len(), 1);
            assert_eq!(left_tables[0].name, "users");
            assert_eq!(left_tables[0].alias, Some("u".to_string()));
            assert_eq!(right_table.name, "orders");
            assert_eq!(right_table.alias, Some("o".to_string()));
        } else {
            panic!("Expected JoinOn context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_multiple_joins() {
        let ctx = context_at_end("SELECT * FROM a JOIN b ON a.id = b.id JOIN c ON ");
        if let CompletionContext::JoinOn {
            left_tables,
            right_table,
            ..
        } = ctx
        {
            // a and b should be in left_tables
            assert_eq!(left_tables.len(), 2);
            assert_eq!(right_table.name, "c");
        } else {
            panic!("Expected JoinOn context, got {:?}", ctx);
        }
    }

    // ========================================
    // GROUP BY, HAVING, ORDER BY tests
    // ========================================

    #[test]
    fn test_group_by() {
        let ctx = context_at_end("SELECT * FROM users GROUP BY ");
        if let CompletionContext::GroupByClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
            assert_eq!(tables[0].name, "users");
        } else {
            panic!("Expected GroupByClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_having() {
        let ctx = context_at_end("SELECT * FROM users GROUP BY name HAVING ");
        if let CompletionContext::HavingClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
        } else {
            panic!("Expected HavingClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_order_by() {
        let ctx = context_at_end("SELECT * FROM users ORDER BY ");
        if let CompletionContext::OrderByClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
        } else {
            panic!("Expected OrderByClause context, got {:?}", ctx);
        }
    }

    // ========================================
    // INSERT tests
    // ========================================

    #[test]
    fn test_insert_into() {
        let ctx = context_at_end("INSERT INTO ");
        assert_eq!(ctx, CompletionContext::AfterInto);
    }

    #[test]
    fn test_insert_columns() {
        let ctx = context_at_end("INSERT INTO users (");
        if let CompletionContext::InsertColumns { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected InsertColumns context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_insert_columns_comma() {
        let ctx = context_at_end("INSERT INTO users (id, ");
        if let CompletionContext::InsertColumns { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected InsertColumns context, got {:?}", ctx);
        }
    }

    // ========================================
    // UPDATE tests
    // ========================================

    #[test]
    fn test_update() {
        let ctx = context_at_end("UPDATE ");
        assert_eq!(ctx, CompletionContext::AfterUpdate);
    }

    #[test]
    fn test_update_set() {
        let ctx = context_at_end("UPDATE users SET ");
        if let CompletionContext::UpdateSet { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected UpdateSet context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_update_set_comma() {
        let ctx = context_at_end("UPDATE users SET name = 'test', ");
        if let CompletionContext::UpdateSet { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected UpdateSet context, got {:?}", ctx);
        }
    }

    // ========================================
    // DELETE tests
    // ========================================

    #[test]
    fn test_delete_from() {
        let ctx = context_at_end("DELETE FROM ");
        assert!(matches!(ctx, CompletionContext::AfterFrom { .. }));
    }

    #[test]
    fn test_delete_where() {
        let ctx = context_at_end("DELETE FROM users WHERE ");
        if let CompletionContext::DeleteWhere { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected DeleteWhere context, got {:?}", ctx);
        }
    }

    // ========================================
    // CREATE tests
    // ========================================

    #[test]
    fn test_after_create() {
        let ctx = context_at_end("CREATE ");
        assert_eq!(ctx, CompletionContext::AfterCreate);
    }

    #[test]
    fn test_create_index_on() {
        let ctx = context_at_end("CREATE INDEX idx ON ");
        assert_eq!(ctx, CompletionContext::AfterOn);
    }

    #[test]
    fn test_create_index_columns() {
        let ctx = context_at_end("CREATE INDEX idx ON users(");
        if let CompletionContext::CreateIndexColumns { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected CreateIndexColumns context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_create_unique_index() {
        let ctx = context_at_end("CREATE UNIQUE INDEX idx ON ");
        assert_eq!(ctx, CompletionContext::AfterOn);
    }

    #[test]
    fn test_create_table_before_table_name() {
        // After CREATE TABLE, before the table name - suggest IF NOT EXISTS
        let ctx = context_at_end("CREATE TABLE ");
        assert_eq!(ctx, CompletionContext::AfterCreateTable);
    }

    #[test]
    fn test_create_table_if_not_exists() {
        // After IF NOT EXISTS, still waiting for table name
        let ctx = context_at_end("CREATE TABLE IF NOT EXISTS ");
        assert_eq!(ctx, CompletionContext::AfterCreateTable);
    }

    #[test]
    fn test_create_table_after_table_name() {
        // After the table name, before opening paren - no specific completions
        let ctx = context_at_end("CREATE TABLE t ");
        assert_eq!(ctx, CompletionContext::None);
    }

    #[test]
    fn test_create_table_inside_parens() {
        // Inside CREATE TABLE column definition area - should return None (no completions)
        let sql = "CREATE TABLE t(\n ";
        let ctx = context_at_end(sql);
        assert_eq!(ctx, CompletionContext::None,
            "Inside CREATE TABLE () should return None");
    }

    #[test]
    fn test_create_table_mdtest_scenario() {
        // Exact SQL from mdtest after marker removal: "create table t(\n \n)"
        // Marker was at position 17 (after "create table t(\n ")
        let sql = "create table t(\n \n)";
        let offset = 17; // position where <ac1> was
        let ctx = detect_context(sql, offset);
        // Inside parentheses should return None
        assert_eq!(ctx, CompletionContext::None);
    }

    // ========================================
    // DROP tests
    // ========================================

    #[test]
    fn test_after_drop() {
        let ctx = context_at_end("DROP ");
        assert_eq!(ctx, CompletionContext::AfterDrop);
    }

    #[test]
    fn test_drop_table() {
        let ctx = context_at_end("DROP TABLE ");
        assert_eq!(ctx, CompletionContext::AfterTable);
    }

    #[test]
    fn test_drop_table_if_exists() {
        let ctx = context_at_end("DROP TABLE IF EXISTS ");
        assert_eq!(ctx, CompletionContext::AfterTable);
    }

    #[test]
    fn test_drop_index() {
        let ctx = context_at_end("DROP INDEX ");
        assert_eq!(ctx, CompletionContext::AfterIndex);
    }

    #[test]
    fn test_drop_view() {
        let ctx = context_at_end("DROP VIEW ");
        assert_eq!(ctx, CompletionContext::AfterView);
    }

    // ========================================
    // ALTER tests
    // ========================================

    #[test]
    fn test_after_alter() {
        let ctx = context_at_end("ALTER ");
        assert_eq!(ctx, CompletionContext::AfterAlter);
    }

    #[test]
    fn test_alter_table() {
        let ctx = context_at_end("ALTER TABLE ");
        assert_eq!(ctx, CompletionContext::AfterTable);
    }

    #[test]
    fn test_alter_table_action() {
        let ctx = context_at_end("ALTER TABLE users ");
        if let CompletionContext::AlterTableAction { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected AlterTableAction context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_alter_table_drop_column() {
        let ctx = context_at_end("ALTER TABLE users DROP COLUMN ");
        if let CompletionContext::AlterColumn { table_name } = ctx {
            assert_eq!(table_name, "users");
        } else {
            panic!("Expected AlterColumn context, got {:?}", ctx);
        }
    }

    // ========================================
    // Edge cases and complex queries
    // ========================================

    #[test]
    fn test_multiple_tables_from() {
        let ctx = context_at_end("SELECT * FROM users, orders WHERE ");
        if let CompletionContext::WhereClause { tables, .. } = ctx {
            // Should have both tables
            assert!(tables.iter().any(|t| t.name == "users"));
            assert!(tables.iter().any(|t| t.name == "orders"));
        } else {
            panic!("Expected WhereClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_subquery_from() {
        // For now, subqueries are not fully tracked
        let ctx = context_at_end("SELECT * FROM (SELECT * FROM users) sub WHERE ");
        // The state machine sees FROM, then (, which doesn't add a table
        // This is a limitation - subquery column inference is Phase 7
        assert!(matches!(ctx, CompletionContext::WhereClause { .. }));
    }

    #[test]
    fn test_context_mid_statement() {
        // Test cursor position in the middle
        let sql = "SELECT id, name FROM users WHERE active = 1";
        let ctx = detect_context(sql, 7); // After "SELECT "
        assert!(matches!(ctx, CompletionContext::SelectColumns { .. }));
    }

    #[test]
    fn test_comments_skipped() {
        let ctx = context_at_end("SELECT -- comment\n* FROM ");
        assert!(matches!(ctx, CompletionContext::AfterFrom { .. }));
    }

    #[test]
    fn test_where_after_join_on() {
        let ctx = context_at_end("SELECT * FROM a JOIN b ON a.id = b.id WHERE ");
        if let CompletionContext::WhereClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 2);
        } else {
            panic!("Expected WhereClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_where_after_identifier() {
        // After typing a column name in WHERE, should suggest operators
        let ctx = context_at_end("SELECT * FROM users WHERE name ");
        assert!(
            matches!(ctx, CompletionContext::AfterWhereExpr { .. }),
            "Expected AfterWhereExpr context, got {:?}",
            ctx
        );
    }

    #[test]
    fn test_where_after_and_suggests_columns() {
        // After AND, should be back in WhereClause for columns
        let ctx = context_at_end("SELECT * FROM users WHERE name = 'test' AND ");
        if let CompletionContext::WhereClause { tables, .. } = ctx {
            assert_eq!(tables.len(), 1);
        } else {
            panic!("Expected WhereClause context, got {:?}", ctx);
        }
    }

    #[test]
    fn test_where_after_complete_expression() {
        // After a complete expression, should suggest AND/OR/etc.
        let ctx = context_at_end("SELECT * FROM users WHERE name = 'test' ");
        assert!(
            matches!(ctx, CompletionContext::AfterWhereExpr { .. }),
            "Expected AfterWhereExpr context, got {:?}",
            ctx
        );
    }

    // ========================================
    // extract_used_insert_columns tests
    // ========================================

    #[test]
    fn test_extract_used_columns_empty() {
        let used = extract_used_insert_columns("INSERT INTO users (", 19);
        assert!(used.is_empty());
    }

    #[test]
    fn test_extract_used_columns_one() {
        let used = extract_used_insert_columns("INSERT INTO users (id, ", 23);
        assert_eq!(used.len(), 1);
        assert!(used.contains("id"));
    }

    #[test]
    fn test_extract_used_columns_multiple() {
        let used = extract_used_insert_columns("INSERT INTO users (id, name, email, ", 36);
        assert_eq!(used.len(), 3);
        assert!(used.contains("id"));
        assert!(used.contains("name"));
        assert!(used.contains("email"));
    }

    #[test]
    fn test_extract_used_columns_case_insensitive() {
        // Should store lowercase versions
        let used = extract_used_insert_columns("INSERT INTO users (ID, Name, ", 29);
        assert_eq!(used.len(), 2);
        assert!(used.contains("id")); // stored as lowercase
        assert!(used.contains("name")); // stored as lowercase
    }

    #[test]
    fn test_extract_used_columns_after_semicolon() {
        // Semicolon should reset the used columns
        let used = extract_used_insert_columns("INSERT INTO t1 (a); INSERT INTO t2 (b, ", 39);
        assert_eq!(used.len(), 1);
        assert!(!used.contains("a")); // from previous statement, should be cleared
        assert!(used.contains("b"));
    }
}
