# Integration Plan: research-sqlite-lexer into solite

## TODO Checklist

- [x] Phase 1: Add solite-ast crate
- [x] Phase 2: Replace solite-lexer with logos-based lexer (in-place)
- [x] Phase 3: Update highlighter for new token types
- [x] Phase 4: Add solite-parser crate
- [x] Phase 5: Add solite-analyzer crate
- [x] Phase 6: Add solite-schema crate
- [x] Phase 7: Add solite-fmt crate
- [x] Phase 8: Add solite-lsp crate
- [x] Phase 9: Add WASM and mdtest infrastructure
- [x] Phase 10: Remove compatibility layer

---

## Overview

Integrate 9 crates from `../research-sqlite-lexer` into the solite project. The new logos-based lexer will be added as `solite-lexer2` to allow parallel existence during the transition. At the end, the old lexer is removed and the new one is renamed.

## Key Constraints

- New lexer added as `solite-lexer2` (parallel to existing `solite-lexer`)
- Preserve existing `json.rs` module (must be copied into lexer2)
- Work iteratively with git commits at each phase
- Tests must pass after each phase: `make test`, `cargo test`
- Crate naming: `solite-*` (hyphens)

## Critical Token API Changes

| Current (`solite-lexer`) | New (`solite-lexer2`) |
|--------------------------|----------------------|
| `Kind` enum (70 variants) | `TokenKind` enum (140+ variants) |
| `Token { kind, start, end, value, contents }` | `Token { kind, span: Range<usize> }` |
| `tokenize(src) -> Vec<Token>` | `lex(source) -> Vec<Token>` |
| `Kind::Number` | `TokenKind::Integer` / `TokenKind::Float` / `TokenKind::HexInteger` |
| `Kind::Comment` | `TokenKind::Comment` (line) / `TokenKind::BlockComment` |
| `Kind::Parameter` | `TokenKind::BindParam*` (4 variants) |
| `Kind::Asterisk` | `TokenKind::Star` |
| `Kind::Ascending/Descending` | `TokenKind::Asc/Desc` |
| `Kind::SingleArrowOperator` | `TokenKind::Arrow` |
| `Kind::DoubleArrowOperator` | `TokenKind::ArrowArrow` |
| `Kind::Div` | `TokenKind::Slash` |

---

## Phase 1: Add solite-ast crate (Independent)

**Goal**: Add AST definitions with no breaking changes.

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_ast/` to `crates/solite-ast/`
- Update `crates/solite-ast/Cargo.toml`: rename package to `solite-ast`

**Files to modify**:
- `Cargo.toml`: Add `"crates/solite-ast"` to workspace members

**Verification**:
```bash
cargo build -p solite-ast && cargo test -p solite-ast && make test
```

**Commit**: `feat: add solite-ast crate with SQL AST definitions`

---

## Phase 2: Add solite-lexer2 (New logos-based lexer)

**Goal**: Add new lexer as `solite-lexer2` alongside existing `solite-lexer`.

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_lexer/` to `crates/solite-lexer2/`
- Copy `crates/solite-lexer/src/json.rs` to `crates/solite-lexer2/src/json.rs` (preserve JSON lexer)

**Files to modify**:

### `crates/solite-lexer2/Cargo.toml`
```toml
[package]
name = "solite-lexer2"
version = "0.1.0"
edition = "2021"

[dependencies]
logos = "0.14"
serde = { version = "1", features = ["derive"] }

[dev-dependencies]
insta = { workspace = true }
```

### `crates/solite-lexer2/src/lib.rs`
Add at the top:
```rust
pub mod json;  // Include JSON module from old lexer
```

### `Cargo.toml` (workspace)
Add `"crates/solite-lexer2"` to members.

**Verification**:
```bash
cargo build -p solite-lexer2 && cargo test -p solite-lexer2 && make test
```

**Commit**: `feat: add solite-lexer2 with logos-based SQL lexer`

---

## Phase 3: Add solite-parser crate

**Goal**: Add the recursive descent + Pratt parser (depends on solite-lexer2).

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_parser/` to `crates/solite-parser/`

**Files to modify**:

### `crates/solite-parser/Cargo.toml`
```toml
[package]
name = "solite-parser"
version = "0.1.0"
edition = "2021"

[dependencies]
solite-lexer2 = { path = "../solite-lexer2" }
solite-ast = { path = "../solite-ast" }
thiserror = "1.0"
ropey = "1.6"

[dev-dependencies]
insta = { workspace = true }
```

### Update internal imports
Change all `solite_lexer` to `solite_lexer2` in source files.

### `Cargo.toml` (workspace)
Add `"crates/solite-parser"` to members.

**Verification**:
```bash
cargo build -p solite-parser && cargo test -p solite-parser && make test
```

**Commit**: `feat: add solite-parser with recursive descent SQL parser`

---

## Phase 4: Add solite-analyzer crate

**Goal**: Add semantic analysis and linting.

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_analyzer/` to `crates/solite-analyzer/`

**Files to modify**:

### `crates/solite-analyzer/Cargo.toml`
```toml
[package]
name = "solite-analyzer"
version = "0.1.0"
edition = "2021"

[dependencies]
solite-ast = { path = "../solite-ast" }
solite-lexer2 = { path = "../solite-lexer2" }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
once_cell = "1"

[dev-dependencies]
solite-parser = { path = "../solite-parser" }
insta = { workspace = true }
```

### Update internal imports
Change all `solite_lexer` to `solite_lexer2`.

### `Cargo.toml` (workspace)
Add `"crates/solite-analyzer"` to members.

**Verification**:
```bash
cargo build -p solite-analyzer && cargo test -p solite-analyzer && make test
```

**Commit**: `feat: add solite-analyzer for semantic analysis and linting`

---

## Phase 5: Add solite-schema crate

**Goal**: Add database schema introspection.

**Note**: Keep rusqlite 0.29 to match existing solite.

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_schema/` to `crates/solite-schema/`

**Files to modify**:

### `crates/solite-schema/Cargo.toml`
```toml
[package]
name = "solite-schema"
version = "0.1.0"
edition = "2021"

[dependencies]
solite-ast = { path = "../solite-ast" }
solite-parser = { path = "../solite-parser" }
solite-analyzer = { path = "../solite-analyzer" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
rusqlite = { workspace = true }

[dev-dependencies]
insta = { workspace = true }
```

### `Cargo.toml` (workspace)
Add `"crates/solite-schema"` to members.

**May need**: Minor adjustments for rusqlite 0.29 API differences.

**Verification**:
```bash
cargo build -p solite-schema && cargo test -p solite-schema && make test
```

**Commit**: `feat: add solite-schema for database introspection`

---

## Phase 6: Add solite-fmt crate

**Goal**: Add SQL formatter with comment preservation.

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_fmt/` to `crates/solite-fmt/`

**Files to modify**:

### `crates/solite-fmt/Cargo.toml`
```toml
[package]
name = "solite-fmt"
version = "0.1.0"
edition = "2021"

[dependencies]
solite-ast = { path = "../solite-ast" }
solite-parser = { path = "../solite-parser" }
solite-lexer2 = { path = "../solite-lexer2" }
solite-schema = { path = "../solite-schema" }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
similar = "2"

[dev-dependencies]
insta = { workspace = true }
```

### Update internal imports
Change all `solite_lexer` to `solite_lexer2`.

### `Cargo.toml` (workspace)
Add `"crates/solite-fmt"` to members.

**Verification**:
```bash
cargo build -p solite-fmt && cargo test -p solite-fmt && make test
```

**Commit**: `feat: add solite-fmt SQL formatter`

---

## Phase 7: Add solite-lsp crate

**Goal**: Add Language Server Protocol implementation.

**Files to create**:
- Copy `../research-sqlite-lexer/crates/solite_lsp/` to `crates/solite-lsp/`

**Files to modify**:

### `crates/solite-lsp/Cargo.toml`
```toml
[package]
name = "solite-lsp"
version = "0.1.0"
edition = "2021"

[dependencies]
solite-lexer2 = { path = "../solite-lexer2" }
solite-parser = { path = "../solite-parser" }
solite-analyzer = { path = "../solite-analyzer" }
solite-ast = { path = "../solite-ast" }
solite-fmt = { path = "../solite-fmt" }
solite-schema = { path = "../solite-schema" }
tower-lsp = "0.20"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
insta = { workspace = true }
```

### Update internal imports
Change all `solite_lexer` to `solite_lexer2`.

### `Cargo.toml` (workspace)
Add `"crates/solite-lsp"` to members.

**Verification**:
```bash
cargo build -p solite-lsp && cargo test -p solite-lsp && make test
```

**Commit**: `feat: add solite-lsp for Language Server Protocol support`

---

## Phase 8: Migrate highlighter/UI to solite-lexer2

**Goal**: Update code that consumes the lexer to use `solite-lexer2`.

**Files to modify**:

### `crates/solite-cli/Cargo.toml`
Add dependency:
```toml
solite-lexer2 = { path = "../solite-lexer2" }
```

### `crates/solite-cli/src/commands/repl/highlighter.rs`

Change import:
```rust
// Old:
use solite_lexer::{tokenize, Kind, Token};

// New:
use solite_lexer2::{lex, TokenKind, Token};
```

Update `highlight_sql()` function:
```rust
pub fn highlight_sql(copy: &mut String) -> String {
    let tokens = lex(copy.as_str());  // Changed from tokenize()
    // ...

    // Update match arms for new token types
    match token.kind {
        TokenKind::Comment | TokenKind::BlockComment => theme.style_comment(&copy[token.span.clone()]),

        TokenKind::BindParam | TokenKind::BindParamColon |
        TokenKind::BindParamAt | TokenKind::BindParamDollar =>
            theme.style_parameter(&copy[token.span.clone()]),

        TokenKind::Integer | TokenKind::Float | TokenKind::HexInteger =>
            theme.style_number(&copy[token.span.clone()]),

        TokenKind::String => theme.style_string(&copy[token.span.clone()]),

        TokenKind::Star | TokenKind::Comma | TokenKind::Semicolon |
        TokenKind::Dot | TokenKind::Ident | TokenKind::QuotedIdent |
        TokenKind::BracketIdent | TokenKind::BacktickIdent =>
            copy[token.span.clone()].to_string(),

        TokenKind::LParen | TokenKind::RParen =>
            theme.style_paren(&copy[token.span.clone()]),

        TokenKind::Arrow | TokenKind::ArrowArrow =>
            theme.style_operator(&copy[token.span.clone()]),

        TokenKind::Plus | TokenKind::Minus | TokenKind::Slash |
        TokenKind::Pipe | TokenKind::Lt | TokenKind::Gt |
        TokenKind::Eq | TokenKind::Ne | /* ... other operators */ =>
            theme.style_operator(&copy[token.span.clone()]),

        // All keywords go to default
        _ => theme.style_keyword(&copy[token.span.clone()]),
    }
}
```

### `crates/solite-cli/src/ui.rs`
Update JSON lexer import (still uses solite-lexer since we copied json.rs to lexer2):
```rust
use solite_lexer2::json::tokenize;
```

### `crates/solite-cli/src/commands/jupyter/server.rs`
Same JSON lexer update.

**Verification**:
```bash
cargo build -p solite-cli && cargo test -p solite-cli
cargo insta review  # Review any snapshot changes
make test
```

**Commit**: `refactor: migrate highlighter and UI to solite-lexer2`

---

## Phase 9 (Optional): Add WASM and test infrastructure

### solite-fmt-wasm
- Copy `../research-sqlite-lexer/crates/solite_fmt_wasm/` to `crates/solite-fmt-wasm/`
- Update dependencies to use `solite-fmt`

### solite-mdtest
- Copy `../research-sqlite-lexer/crates/solite_mdtest/` to `crates/solite-mdtest/`

**Verification**:
```bash
cargo build -p solite-fmt-wasm --target wasm32-unknown-unknown
cargo test -p solite-mdtest
make test
```

**Commit**: `feat: add WASM formatter and mdtest infrastructure`

---

## Phase 10: Remove solite-lexer (old)

**Goal**: Remove the old lexer crate now that everything uses solite-lexer2.

**Files to delete**:
- `crates/solite-lexer/` (entire directory)

**Files to modify**:
- `Cargo.toml`: Remove `"crates/solite-lexer"` from workspace members
- `crates/solite-cli/Cargo.toml`: Remove `solite-lexer` dependency (if still there)

**Verification**:
```bash
cargo build && cargo test && make test
```

**Commit**: `chore: remove old solite-lexer crate`

---

## Phase 11: Rename solite-lexer2 -> solite-lexer

**Goal**: Rename the new lexer to the canonical name.

**Files to rename**:
- `crates/solite-lexer2/` -> `crates/solite-lexer/`

**Files to modify**:

### `crates/solite-lexer/Cargo.toml`
```toml
name = "solite-lexer"  # Was "solite-lexer2"
```

### `Cargo.toml` (workspace)
Change `"crates/solite-lexer2"` to `"crates/solite-lexer"` in members.

### All dependent crates
Update Cargo.toml in each crate:
- `solite-parser/Cargo.toml`: `solite-lexer2` -> `solite-lexer`
- `solite-analyzer/Cargo.toml`: `solite-lexer2` -> `solite-lexer`
- `solite-fmt/Cargo.toml`: `solite-lexer2` -> `solite-lexer`
- `solite-lsp/Cargo.toml`: `solite-lexer2` -> `solite-lexer`
- `solite-cli/Cargo.toml`: `solite-lexer2` -> `solite-lexer`

### All source files
Update imports in all `.rs` files:
```rust
// Old:
use solite_lexer2::...

// New:
use solite_lexer::...
```

**Verification**:
```bash
cargo build && cargo test && make test
```

**Commit**: `refactor: rename solite-lexer2 to solite-lexer`

---

## Files Summary

| Phase | New Crates | Modified Files |
|-------|------------|----------------|
| 1 | `solite-ast` | `Cargo.toml` |
| 2 | `solite-lexer2` | `Cargo.toml`, copy `json.rs` |
| 3 | `solite-parser` | `Cargo.toml` |
| 4 | `solite-analyzer` | `Cargo.toml` |
| 5 | `solite-schema` | `Cargo.toml` |
| 6 | `solite-fmt` | `Cargo.toml` |
| 7 | `solite-lsp` | `Cargo.toml` |
| 8 | - | `solite-cli/` highlighter, ui, jupyter |
| 9 | `solite-fmt-wasm`, `solite-mdtest` | `Cargo.toml` |
| 10 | - | Remove `solite-lexer/` |
| 11 | - | Rename `solite-lexer2/` -> `solite-lexer/` |

---

## Verification Strategy

After each phase:
1. `cargo build` - Ensure compilation
2. `cargo test` - Run Rust unit tests
3. `cargo insta review` - Review snapshot changes (if any)
4. `make test` - Run full test suite including Python integration tests
5. Git commit on success

---

## Risk Mitigation

1. **Parallel existence**: Old and new lexer coexist until Phase 10, allowing gradual migration
2. **Snapshot test failures**: Use `cargo insta review` to update expected snapshots
3. **rusqlite version**: Keep 0.29 to match existing solite; adjust solite-schema if needed
4. **JSON lexer**: Copied from old lexer to new, functionality preserved

---

## Completion Report

All 10 phases are complete. The implementation replaced `solite-lexer` in-place (with a compatibility layer) rather than creating a parallel `solite-lexer2` crate.

| Phase | Commit | Description |
|-------|--------|-------------|
| 1 | `a758ba0` | Add solite-ast crate |
| 2 | `6e8ae74` | Replace solite-lexer with logos-based lexer |
| 3 | (included in 2) | Update highlighter for new token types |
| 4 | `5dd9d97` | Add solite-parser |
| 5 | `734199f` | Add solite-analyzer |
| 6 | `3fb20f5` | Add solite-schema |
| 7 | `c10e6b3` | Add solite-fmt |
| 8 | `a0618de` | Add solite-lsp |
| 9 | `e9a17aa` | Add WASM and mdtest infrastructure |
| 10 | `5611e2e` | Remove compatibility layer |

Additional fix during testing:
- `c95dbdc` - Fix whitespace preservation in REPL and Jupyter syntax highlighting

The new lexer API is now:
- `lex(source: &str) -> Vec<Token>` where `Token { kind: TokenKind, span: Range<usize> }`
- `TokenKind` enum with 140+ variants for all SQL tokens
- JSON lexer preserved in `solite_lexer::json` module

---

# Unify REPL and LSP Completion Systems

## TODO Checklist

- [x] Phase 1: Create solite-completion crate with context detection
- [x] Phase 2: Add abstract completion types (items.rs, schema.rs)
- [x] Phase 3: Move completion generation logic (engine.rs)
- [x] Phase 4: Update solite-lsp to use solite-completion
- [x] Phase 5: Update REPL completer to use solite-completion

---

## Overview

Created a new `solite-completion` crate that extracts the context-aware completion logic from `solite-lsp`, allowing both the LSP and REPL to share the same completion engine.

## Architecture

```
solite-completion (NEW)
├── src/
│   ├── lib.rs        # Exports
│   ├── context.rs    # CompletionContext detection (from LSP)
│   ├── items.rs      # Abstract CompletionItem, CompletionKind
│   ├── engine.rs     # get_completions(context, schema)
│   └── schema.rs     # SchemaSource trait

solite-lsp
└── Uses solite-completion, converts to LSP types

solite-cli (REPL)
└── Uses solite-completion, converts to rustyline Pairs
```

## Key Types

### CompletionItem (in solite-completion)
```rust
pub struct CompletionItem {
    pub label: String,
    pub insert_text: Option<String>,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub sort_order: Option<u32>,
}

pub enum CompletionKind {
    Keyword, Table, Column, Index, View, Function, Operator, Cte,
}
```

### SchemaSource trait
```rust
pub trait SchemaSource {
    fn table_names(&self) -> Vec<String>;
    fn columns_for_table(&self, table: &str) -> Option<Vec<String>>;
    fn columns_for_table_with_rowid(&self, table: &str) -> Option<Vec<String>>;
    fn has_table(&self, name: &str) -> bool;
    fn index_names(&self) -> Vec<String>;
    fn view_names(&self) -> Vec<String>;
}
```

## What Stays Where

**In solite-completion (shared):**
- Context detection (CompletionContext, state machine)
- Abstract CompletionItem type
- Core completion generation logic
- SchemaSource trait
- Implementation for solite_analyzer::Schema (behind feature flag)

**In solite-lsp (LSP-specific):**
- Conversion to `tower_lsp::lsp_types::CompletionItem`
- Documentation strings for keywords
- Snippet generation (InsertTextFormat)
- CompletionOptions

**In solite-cli REPL (REPL-specific):**
- Dot command completion (`.tables`, `.load`, etc.)
- Conversion to `rustyline::completion::Pair`
- Display formatting (icons, colors)
- LiveSchemaSource implementation (queries live SQLite database)
