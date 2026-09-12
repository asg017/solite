//! Shared SQL token -> theme role classification.
//!
//! Extracted from the REPL highlighter so the REPL (ANSI) and the Jupyter
//! HTML renderer paint the exact same token the exact same role — before
//! this module existed the two had quietly drifted (parameters were YELLOW
//! in the Jupyter renderer but MAROON in the REPL; comments were OVERLAY0 vs
//! OVERLAY2; blob literals were grouped with numbers in one and strings in
//! the other). [`classify`] is now the single source of truth; the REPL maps
//! [`Role`] to a [`Style`] and paints with ANSI escapes
//! (`commands/repl/highlighter.rs`), the Jupyter renderer maps it to a CSS
//! `color` declaration (`commands/jupyter/render/syntax.rs`).

use solite_lexer::TokenKind;
use solite_stdlib::BUILTIN_FUNCTIONS;
use solite_theme::{Style, Theme};

/// The semantic role a SQL token is painted with. Deliberately smaller than
/// [`Theme`]'s full role set: only the roles a SQL *token* (as opposed to a
/// cell value or a dot command) can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Keyword,
    StringLiteral,
    Comment,
    Parameter,
    Operator,
    Number,
    Function,
    FunctionBuiltin,
    Punctuation,
}

impl Role {
    /// The [`Style`] this role paints with, in `theme`.
    pub fn style(self, theme: &Theme) -> &Style {
        match self {
            Role::Keyword => &theme.keyword,
            Role::StringLiteral => &theme.string_literal,
            Role::Comment => &theme.comment,
            Role::Parameter => &theme.parameter,
            Role::Operator => &theme.operator,
            Role::Number => &theme.number,
            Role::Function => &theme.function,
            Role::FunctionBuiltin => &theme.function_builtin,
            Role::Punctuation => &theme.punctuation,
        }
    }
}

/// Classify one token. Returns `None` for tokens that should render as
/// plain, unstyled text — bare punctuation (`*`, `,`, `;`, `.`, brackets)
/// and plain identifiers (columns, tables, aliases): coloring every
/// identifier and comma made both the REPL and Jupyter output noisier
/// without adding information, so neither renderer paints them.
///
/// `prev_kind` is the immediately preceding token (for the `USING (...)` /
/// `TABLE (...)` exclusion below); `next_is_lparen` is whether the next
/// token is `(` (the function-call heuristic); `text` is the token's source
/// text, trimmed and compared against [`BUILTIN_FUNCTIONS`] to distinguish
/// [`Role::FunctionBuiltin`] from [`Role::Function`].
pub fn classify(
    kind: TokenKind,
    text: &str,
    prev_kind: Option<TokenKind>,
    next_is_lparen: bool,
) -> Option<Role> {
    use TokenKind::*;
    match kind {
        // Comments (line and block)
        Comment | BlockComment => Some(Role::Comment),
        // Bind parameters (all 4 variants)
        BindParam | BindParamColon | BindParamAt | BindParamDollar => Some(Role::Parameter),
        // Numbers (integer, float, hex)
        Integer | Float | HexInteger => Some(Role::Number),
        // Operators
        Plus | Minus | Pipe | Slash | Lt | Gt | Le | Ge | Eq | EqEq | Ne | BangEq | Ampersand
        | Tilde | LShift | RShift | Concat | Percent
        // JSON operators
        | Arrow | ArrowArrow => Some(Role::Operator),
        // String and blob literals
        String | Blob => Some(Role::StringLiteral),
        // Bare punctuation: no styling
        Star | LBracket | RBracket | Comma | Semicolon | Dot => None,
        // Parentheses
        LParen | RParen => Some(Role::Punctuation),
        // Identifiers (regular and quoted): a function role only when
        // immediately followed by `(` and not preceded by `USING`/`TABLE`
        // (e.g. `USING (col)` or `CREATE TABLE (...)` aren't calls).
        Ident | QuotedIdent | BracketIdent | BacktickIdent => {
            if next_is_lparen && !matches!(prev_kind, Some(Using) | Some(Table)) {
                if BUILTIN_FUNCTIONS.iter().any(|f| *f == text.trim()) {
                    Some(Role::FunctionBuiltin)
                } else {
                    Some(Role::Function)
                }
            } else {
                None
            }
        }
        // Everything else is a keyword
        _ => Some(Role::Keyword),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_all(sql: &str) -> Vec<(String, Option<Role>)> {
        let tokens = solite_lexer::lex(sql);
        let mut prev_kind = None;
        let mut out = vec![];
        for (i, token) in tokens.iter().enumerate() {
            let next_is_lparen =
                matches!(tokens.get(i + 1).map(|t| t.kind), Some(TokenKind::LParen));
            let text = &sql[token.span.clone()];
            out.push((
                text.to_string(),
                classify(token.kind, text, prev_kind, next_is_lparen),
            ));
            prev_kind = Some(token.kind);
        }
        out
    }

    #[test]
    fn bind_parameters_are_parameter_role() {
        let result = classify_all("select $x, :y, @z, ?1 from t;");
        assert!(result
            .iter()
            .filter(|(t, _)| ["$x", ":y", "@z", "?1"].contains(&t.as_str()))
            .all(|(_, r)| *r == Some(Role::Parameter)));
    }

    #[test]
    fn comments_are_comment_role() {
        let result = classify_all("-- hi\nselect 1; /* block */");
        assert_eq!(
            result
                .iter()
                .find(|(t, _)| t.starts_with("--"))
                .map(|(_, r)| *r),
            Some(Some(Role::Comment))
        );
        assert_eq!(
            result
                .iter()
                .find(|(t, _)| t.starts_with("/*"))
                .map(|(_, r)| *r),
            Some(Some(Role::Comment))
        );
    }

    #[test]
    fn blob_literal_is_string_literal_role_not_number() {
        let result = classify_all("select X'CAFE';");
        let (_, role) = result.iter().find(|(t, _)| t.starts_with('X')).unwrap();
        assert_eq!(*role, Some(Role::StringLiteral));
    }

    #[test]
    fn builtin_function_is_function_builtin_role() {
        let result = classify_all("select sqlite_version();");
        let (_, role) = result
            .iter()
            .find(|(t, _)| t == "sqlite_version")
            .unwrap();
        assert_eq!(*role, Some(Role::FunctionBuiltin));
    }

    #[test]
    fn using_clause_column_is_not_a_function_call() {
        let result = classify_all("select * from a join b using (id);");
        let (_, role) = result.iter().find(|(t, _)| t == "id").unwrap();
        assert_eq!(*role, None);
    }

    #[test]
    fn bare_punctuation_and_identifiers_are_unstyled() {
        let result = classify_all("select a, b from t;");
        for name in ["a", "b", "t", ",", ";"] {
            let (_, role) = result.iter().find(|(t, _)| t == name).unwrap();
            assert_eq!(*role, None, "{name:?} should be unstyled");
        }
    }
}
