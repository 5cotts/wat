/// A single lexical token produced by the shell lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A word (bare, single-quoted, double-quoted, or a mix joined together).
    Word(String),
    /// `|`
    Pipe,
    /// `<`
    RedirectIn,
    /// `>`
    RedirectOut,
    /// `>>`
    RedirectAppend,
    /// `;`
    Semicolon,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `\n`
    Newline,
    /// End of input.
    Eof,
}

impl Token {
    /// Returns the canonical text representation of the token (used for round-trip tests).
    pub fn display(&self) -> &str {
        match self {
            Token::Pipe => "|",
            Token::RedirectIn => "<",
            Token::RedirectOut => ">",
            Token::RedirectAppend => ">>",
            Token::Semicolon => ";",
            Token::And => "&&",
            Token::Or => "||",
            Token::Newline => "\n",
            Token::Eof => "",
            Token::Word(s) => s.as_str(),
        }
    }
}

/// A token paired with its byte offset in the source string.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub token: Token,
    /// Byte offset of the first character of this token.
    pub offset: usize,
}

/// Errors that can occur during lexing.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lex error at offset {}: {}", self.offset, self.message)
    }
}

/// Tokenize a shell input string into a list of [`Spanned`] tokens ending with [`Token::Eof`].
pub fn lex(input: &str) -> Result<Vec<Spanned>, LexError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    // Byte offset tracking (chars may be multi-byte).
    let byte_offsets: Vec<usize> = {
        let mut offsets = Vec::with_capacity(chars.len() + 1);
        let mut off = 0;
        for &c in &chars {
            offsets.push(off);
            off += c.len_utf8();
        }
        offsets.push(off);
        offsets
    };

    macro_rules! byte_off {
        ($idx:expr) => {
            byte_offsets[$idx]
        };
    }

    while i < chars.len() {
        let c = chars[i];
        let offset = byte_off!(i);

        match c {
            // Skip horizontal whitespace
            ' ' | '\t' => {
                i += 1;
            }
            '\n' => {
                tokens.push(Spanned { token: Token::Newline, offset });
                i += 1;
            }
            '|' => {
                if i + 1 < chars.len() && chars[i + 1] == '|' {
                    tokens.push(Spanned { token: Token::Or, offset });
                    i += 2;
                } else {
                    tokens.push(Spanned { token: Token::Pipe, offset });
                    i += 1;
                }
            }
            '&' => {
                if i + 1 < chars.len() && chars[i + 1] == '&' {
                    tokens.push(Spanned { token: Token::And, offset });
                    i += 2;
                } else {
                    // Lone `&` — treat as word for now (background jobs not supported)
                    tokens.push(Spanned { token: Token::Word("&".to_string()), offset });
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(Spanned { token: Token::RedirectAppend, offset });
                    i += 2;
                } else {
                    tokens.push(Spanned { token: Token::RedirectOut, offset });
                    i += 1;
                }
            }
            '<' => {
                tokens.push(Spanned { token: Token::RedirectIn, offset });
                i += 1;
            }
            ';' => {
                tokens.push(Spanned { token: Token::Semicolon, offset });
                i += 1;
            }
            '#' => {
                // Comment: skip to end of line
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\'' => {
                // Single-quoted: everything literal until closing '
                i += 1;
                let start = i;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '\'' {
                    s.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(LexError {
                        message: "unterminated single quote".to_string(),
                        offset: byte_off!(start - 1),
                    });
                }
                i += 1; // consume closing '
                tokens.push(Spanned { token: Token::Word(s), offset });
            }
            '"' => {
                // Double-quoted: allow backslash escapes, defer $ expansion
                i += 1;
                let start = i;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                        match chars[i] {
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            '$' => s.push('$'),
                            other => {
                                s.push('\\');
                                s.push(other);
                            }
                        }
                        i += 1;
                    } else {
                        s.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(LexError {
                        message: "unterminated double quote".to_string(),
                        offset: byte_off!(start - 1),
                    });
                }
                i += 1; // consume closing "
                tokens.push(Spanned { token: Token::Word(s), offset });
            }
            '\\' => {
                // Backslash outside quotes: escape next character
                if i + 1 < chars.len() {
                    i += 1;
                    let escaped = chars[i];
                    i += 1;
                    // May be the start of a longer word — try to join with following word chars
                    let mut s = String::new();
                    s.push(escaped);
                    s = extend_word(s, &chars, &byte_offsets, &mut i, &mut tokens);
                    tokens.push(Spanned { token: Token::Word(s), offset });
                } else {
                    return Err(LexError {
                        message: "trailing backslash".to_string(),
                        offset,
                    });
                }
            }
            _ => {
                // Bare word
                let mut s = String::new();
                s.push(c);
                i += 1;
                s = extend_word(s, &chars, &byte_offsets, &mut i, &mut tokens);
                tokens.push(Spanned { token: Token::Word(s), offset });
            }
        }
    }

    tokens.push(Spanned { token: Token::Eof, offset: input.len() });
    Ok(tokens)
}

/// Continue accumulating characters into a word, handling embedded quotes and escapes.
/// Stops at unquoted whitespace or operator characters.
fn extend_word(
    mut s: String,
    chars: &[char],
    byte_offsets: &[usize],
    i: &mut usize,
    _tokens: &mut Vec<Spanned>,
) -> String {
    while *i < chars.len() {
        match chars[*i] {
            ' ' | '\t' | '\n' | '|' | '&' | '<' | '>' | ';' => break,
            '\'' => {
                *i += 1;
                while *i < chars.len() && chars[*i] != '\'' {
                    s.push(chars[*i]);
                    *i += 1;
                }
                if *i < chars.len() {
                    *i += 1; // consume closing '
                }
            }
            '"' => {
                *i += 1;
                while *i < chars.len() && chars[*i] != '"' {
                    if chars[*i] == '\\' && *i + 1 < chars.len() {
                        *i += 1;
                        match chars[*i] {
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            '$' => s.push('$'),
                            other => {
                                s.push('\\');
                                s.push(other);
                            }
                        }
                        *i += 1;
                    } else {
                        s.push(chars[*i]);
                        *i += 1;
                    }
                }
                if *i < chars.len() {
                    *i += 1; // consume closing "
                }
            }
            '\\' if *i + 1 < chars.len() => {
                *i += 1;
                s.push(chars[*i]);
                *i += 1;
            }
            c => {
                s.push(c);
                *i += 1;
            }
        }
        let _ = byte_offsets; // suppress unused warning
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token> {
        lex(input).unwrap().into_iter().map(|s| s.token).collect()
    }

    #[test]
    fn empty_input() {
        assert_eq!(tokens(""), vec![Token::Eof]);
    }

    #[test]
    fn single_word() {
        assert_eq!(tokens("echo"), vec![Token::Word("echo".into()), Token::Eof]);
    }

    #[test]
    fn pipe() {
        assert_eq!(
            tokens("a | b"),
            vec![Token::Word("a".into()), Token::Pipe, Token::Word("b".into()), Token::Eof]
        );
    }

    #[test]
    fn and_or() {
        assert_eq!(
            tokens("a && b || c"),
            vec![
                Token::Word("a".into()),
                Token::And,
                Token::Word("b".into()),
                Token::Or,
                Token::Word("c".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn redirects() {
        assert_eq!(
            tokens("cat < in > out >> app"),
            vec![
                Token::Word("cat".into()),
                Token::RedirectIn,
                Token::Word("in".into()),
                Token::RedirectOut,
                Token::Word("out".into()),
                Token::RedirectAppend,
                Token::Word("app".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn semicolon() {
        assert_eq!(
            tokens("a ; b"),
            vec![Token::Word("a".into()), Token::Semicolon, Token::Word("b".into()), Token::Eof]
        );
    }

    #[test]
    fn single_quoted() {
        assert_eq!(tokens("'hello world'"), vec![Token::Word("hello world".into()), Token::Eof]);
    }

    #[test]
    fn double_quoted() {
        assert_eq!(
            tokens("\"hello world\""),
            vec![Token::Word("hello world".into()), Token::Eof]
        );
    }

    #[test]
    fn double_quoted_escape() {
        assert_eq!(tokens(r#""a\"b""#), vec![Token::Word(r#"a"b"#.into()), Token::Eof]);
    }

    #[test]
    fn backslash_escape_bare() {
        assert_eq!(tokens(r"a\ b"), vec![Token::Word("a b".into()), Token::Eof]);
    }

    #[test]
    fn mixed_quoting() {
        assert_eq!(
            tokens(r#"hel'lo '"world""#),
            vec![Token::Word("hello world".into()), Token::Eof]
        );
    }

    #[test]
    fn comment_skipped() {
        assert_eq!(tokens("echo hi # ignored"), vec![
            Token::Word("echo".into()),
            Token::Word("hi".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn newline_token() {
        assert_eq!(tokens("a\nb"), vec![
            Token::Word("a".into()),
            Token::Newline,
            Token::Word("b".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn full_pipeline_example() {
        let toks = tokens(r#"echo "hello world" | grep h > out.txt && cat out.txt"#);
        assert_eq!(toks, vec![
            Token::Word("echo".into()),
            Token::Word("hello world".into()),
            Token::Pipe,
            Token::Word("grep".into()),
            Token::Word("h".into()),
            Token::RedirectOut,
            Token::Word("out.txt".into()),
            Token::And,
            Token::Word("cat".into()),
            Token::Word("out.txt".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn unterminated_single_quote_error() {
        assert!(lex("'unterminated").is_err());
    }

    #[test]
    fn unterminated_double_quote_error() {
        assert!(lex("\"unterminated").is_err());
    }

    #[test]
    fn trailing_backslash_error() {
        assert!(lex("echo \\").is_err());
    }

    #[test]
    fn offset_tracking() {
        let spanned = lex("a | b").unwrap();
        assert_eq!(spanned[0].offset, 0); // 'a'
        assert_eq!(spanned[1].offset, 2); // '|'
        assert_eq!(spanned[2].offset, 4); // 'b'
    }
}
