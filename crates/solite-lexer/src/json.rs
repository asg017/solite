use std::str::Chars;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum StringContext {
    /// String is an object key
    Key,
    /// String is a value (in object or array)
    Value,
    /// Context is unknown (shouldn't happen in valid JSON)
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Token<'a> {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// For String tokens, indicates if it's a key or value. None for non-string tokens.
    pub string_context: Option<StringContext>,
    /// Reference to the actual token text in the source
    pub text: &'a str,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Eof,         // end of file
    LBrace,      // {
    RBrace,      // }
    LBracket,    // [
    RBracket,    // ]
    Colon,       // :
    Comma,       // ,
    String,      // "..."
    Number,      // 123, -45.6, 1.2e-10
    True,        // true
    False,       // false
    Null,        // null
    Whitespace,  // spaces, tabs, newlines
    Unknown,     // invalid characters
}

struct Lexer<'a> {
    source: &'a str,
    chars: Chars<'a>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars(),
        }
    }
}

impl<'a> Lexer<'a> {
    fn read_next_kind(&mut self) -> Kind {
        while let Some(c) = self.chars.next() {
            match c {
                '{' => return Kind::LBrace,
                '}' => return Kind::RBrace,
                '[' => return Kind::LBracket,
                ']' => return Kind::RBracket,
                ':' => return Kind::Colon,
                ',' => return Kind::Comma,
                '"' => {
                    // Parse string
                    loop {
                        match self.next() {
                            Some('\\') => {
                                // Skip escaped character
                                self.next();
                            }
                            Some('"') => break,
                            Some(_) => continue,
                            None => break,
                        }
                    }
                    return Kind::String;
                }
                '-' | '0'..='9' => {
                    // Parse number
                    // Can be: integer, decimal, or exponential notation
                    while let Some(ch) = self.peek() {
                        match ch {
                            '0'..='9' | '.' | 'e' | 'E' | '+' | '-' => {
                                self.next();
                            }
                            _ => break,
                        }
                    }
                    return Kind::Number;
                }
                't' => {
                    // Check for 'true'
                    if self.peek() == Some('r') {
                        self.next();
                        if self.peek() == Some('u') {
                            self.next();
                            if self.peek() == Some('e') {
                                self.next();
                                return Kind::True;
                            }
                        }
                    }
                    return Kind::Unknown;
                }
                'f' => {
                    // Check for 'false'
                    if self.peek() == Some('a') {
                        self.next();
                        if self.peek() == Some('l') {
                            self.next();
                            if self.peek() == Some('s') {
                                self.next();
                                if self.peek() == Some('e') {
                                    self.next();
                                    return Kind::False;
                                }
                            }
                        }
                    }
                    return Kind::Unknown;
                }
                'n' => {
                    // Check for 'null'
                    if self.peek() == Some('u') {
                        self.next();
                        if self.peek() == Some('l') {
                            self.next();
                            if self.peek() == Some('l') {
                                self.next();
                                return Kind::Null;
                            }
                        }
                    }
                    return Kind::Unknown;
                }
                ' ' | '\n' | '\t' | '\r' => {
                    // Consume all consecutive whitespace
                    while let Some(ch) = self.peek() {
                        match ch {
                            ' ' | '\n' | '\t' | '\r' => {
                                self.next();
                            }
                            _ => break,
                        }
                    }
                    return Kind::Whitespace;
                }
                _ => return Kind::Unknown,
            }
        }
        Kind::Eof
    }

    fn read_next_token(&mut self, string_context: Option<StringContext>) -> Token<'a> {
        let start = self.offset();
        let kind = self.read_next_kind();
        let end = self.offset();
        let text = &self.source[start..end];
        
        Token {
            kind,
            start,
            end,
            string_context,
            text,
        }
    }

    /// Get the length offset from the source text, in UTF-8 bytes
    fn offset(&self) -> usize {
        self.source.len() - self.chars.as_str().len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.clone().next()
    }

    fn next(&mut self) -> Option<char> {
        self.chars.next()
    }
}

pub fn tokenize<'a>(src: &'a str) -> Vec<Token<'a>> {
    let mut l = Lexer::new(src);
    let mut tokens = vec![];
    
    // Track context: are we expecting a key next?
    // Stack to handle nesting: true = in object (track keys), false = in array
    let mut context_stack: Vec<bool> = vec![];
    let mut expecting_key = false;
    
    loop {
        // Determine string context based on current state
        let string_context = if expecting_key {
            Some(StringContext::Key)
        } else if !context_stack.is_empty() {
            Some(StringContext::Value)
        } else {
            None
        };
        
        let token = l.read_next_token(string_context);
        let should_break = token.kind == Kind::Eof;
        
        // Update context based on the token we just read
        match token.kind {
            Kind::LBrace => {
                // Entering object, next string should be a key
                context_stack.push(true);
                expecting_key = true;
            }
            Kind::LBracket => {
                // Entering array, strings are values
                context_stack.push(false);
                expecting_key = false;
            }
            Kind::RBrace | Kind::RBracket => {
                // Exiting object or array
                context_stack.pop();
                expecting_key = context_stack.last() == Some(&true);
            }
            Kind::Colon => {
                // After colon in object, next value is a value (not a key)
                expecting_key = false;
            }
            Kind::Comma => {
                // After comma, determine context from current container
                expecting_key = context_stack.last() == Some(&true);
            }
            Kind::String | Kind::Number | Kind::True | Kind::False | Kind::Null => {
                // After a value in an object, we don't immediately expect a key
                // (need a comma first)
                if context_stack.last() == Some(&true) && expecting_key {
                    // We just read a key, don't expect another key until after comma
                    expecting_key = false;
                }
            }
            _ => {}
        }
        
        tokens.push(token);
        if should_break {
            break;
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_lexer() {
        let tests = vec![
            r#"{"name": "John", "age": 30}"#,
            r#"[1, 2, 3, 4, 5]"#,
            r#"{"active": true, "inactive": false, "data": null}"#,
            r#"{"number": 123, "float": -45.67, "exp": 1.2e-10}"#,
            r#"{
    "nested": {
        "array": [1, 2, 3],
        "bool": true
    }
}"#,
        ];
        for (i, test) in tests.iter().enumerate() {
            let tokens = tokenize(test);
            let v: Vec<String> = tokens
                .iter()
                .map(|t| (&test[t.start..t.end]).to_string())
                .collect();
            let result: Vec<(&String, &Token)> = v.iter().zip(&tokens).collect();
            insta::assert_debug_snapshot!(format!("json_test_{i}"), result);
        }
    }

    #[test]
    fn test_json_kinds() {
        let src = r#"{"key": "value", "num": 42, "bool": true, "null": null}"#;
        let tokens = tokenize(src);
        
        // Filter out whitespace for easier assertion
        let kinds: Vec<Kind> = tokens.iter().map(|t| t.kind).collect();
        
        assert!(kinds.contains(&Kind::LBrace));
        assert!(kinds.contains(&Kind::RBrace));
        assert!(kinds.contains(&Kind::Colon));
        assert!(kinds.contains(&Kind::Comma));
        assert!(kinds.contains(&Kind::String));
        assert!(kinds.contains(&Kind::Number));
        assert!(kinds.contains(&Kind::True));
        assert!(kinds.contains(&Kind::Null));
    }

    #[test]
    fn test_string_with_escapes() {
        let src = r#"{"text": "Hello \"World\""}"#;
        let tokens = tokenize(src);
        let string_tokens: Vec<&Token> = tokens.iter().filter(|t| t.kind == Kind::String).collect();
        assert_eq!(string_tokens.len(), 2);
    }

    #[test]
    fn test_numbers() {
        let src = r#"[123, -456, 78.9, -0.12, 1e10, 2.5e-3]"#;
        let tokens = tokenize(src);
        let number_tokens: Vec<&Token> = tokens.iter().filter(|t| t.kind == Kind::Number).collect();
        assert_eq!(number_tokens.len(), 6);
    }
}
