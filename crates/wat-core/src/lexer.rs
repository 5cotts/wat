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
    /// `2>`
    Redirect2Out,
    /// `2>>`
    Redirect2Append,
    /// `;`
    Semicolon,
    /// `;;` (case-arm terminator)
    DSemi,
    /// `&&`
    And,
    /// `&`
    Background,
    /// `||`
    Or,
    /// `\n`
    Newline,
    /// A here-document body, already collected by the lexer. The `bool` is
    /// whether the body should be expanded (`false` for a quoted delimiter).
    HereDoc(String, bool),
    /// The `<<<` here-string operator; the following word is its content.
    HereStringOp,
    /// End of input.
    Eof,
}

impl Token {
    /// Returns the canonical text representation of the token (used for round-trip tests).
    pub fn display(&self) -> &str {
        match self {
            Token::Redirect2Out => "2>",
            Token::Redirect2Append => "2>>",
            Token::Pipe => "|",
            Token::RedirectIn => "<",
            Token::RedirectOut => ">",
            Token::RedirectAppend => ">>",
            Token::Semicolon => ";",
            Token::DSemi => ";;",
            Token::And => "&&",
            Token::Background => "&",
            Token::Or => "||",
            Token::Newline => "\n",
            Token::HereDoc(_, _) => "<<",
            Token::HereStringOp => "<<<",
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

/// Internal marker the lexer prefixes onto a command/arith substitution span
/// it saw inside double quotes. The expander (Tier 4 Phase C) honors it ONLY
/// when it immediately precedes a `$(`/backtick span the expander recognizes,
/// to decide that the substitution's output must not be word-split. A lone
/// marker elsewhere is an ordinary character and round-trips literally. (Same
/// idea as bash's internal CTLESC quoting bytes.)
pub const QUOTED_SUBST_MARK: char = '\u{1}';

/// Tokenize a shell input string into a list of [`Spanned`] tokens ending with [`Token::Eof`].
pub fn lex(input: &str) -> Result<Vec<Spanned>, LexError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    // Here-docs awaiting their bodies: (index of the placeholder HereDoc token,
    // delimiter, strip-leading-tabs). Filled when the terminating newline of the
    // line that introduced them is reached.
    let mut pending: Vec<(usize, String, bool)> = Vec::new();

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
                tokens.push(Spanned {
                    token: Token::Newline,
                    offset,
                });
                i += 1;
                // The line that opened any here-docs has ended: collect each
                // body (in order) from the following lines.
                if !pending.is_empty() {
                    for (idx, delim, strip) in std::mem::take(&mut pending) {
                        let body = collect_heredoc_body(&chars, &mut i, &delim, strip)?;
                        if let Token::HereDoc(ref mut b, _) = tokens[idx].token {
                            *b = body;
                        }
                    }
                }
            }
            '|' => {
                if i + 1 < chars.len() && chars[i + 1] == '|' {
                    tokens.push(Spanned {
                        token: Token::Or,
                        offset,
                    });
                    i += 2;
                } else {
                    tokens.push(Spanned {
                        token: Token::Pipe,
                        offset,
                    });
                    i += 1;
                }
            }
            '&' => {
                if i + 1 < chars.len() && chars[i + 1] == '&' {
                    tokens.push(Spanned {
                        token: Token::And,
                        offset,
                    });
                    i += 2;
                } else {
                    tokens.push(Spanned {
                        token: Token::Background,
                        offset,
                    });
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(Spanned {
                        token: Token::RedirectAppend,
                        offset,
                    });
                    i += 2;
                } else {
                    tokens.push(Spanned {
                        token: Token::RedirectOut,
                        offset,
                    });
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '<' {
                    if i + 2 < chars.len() && chars[i + 2] == '<' {
                        // `<<<` here-string: the following word is the content.
                        i += 3;
                        tokens.push(Spanned {
                            token: Token::HereStringOp,
                            offset,
                        });
                    } else {
                        // `<<` / `<<-` here-document.
                        let strip = i + 2 < chars.len() && chars[i + 2] == '-';
                        i += if strip { 3 } else { 2 };
                        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                            i += 1;
                        }
                        let (delim, expand) = read_heredoc_delim(&chars, &mut i)?;
                        let idx = tokens.len();
                        tokens.push(Spanned {
                            token: Token::HereDoc(String::new(), expand),
                            offset,
                        });
                        pending.push((idx, delim, strip));
                    }
                } else {
                    tokens.push(Spanned {
                        token: Token::RedirectIn,
                        offset,
                    });
                    i += 1;
                }
            }
            ';' => {
                // `;;` is the case-arm terminator (one token); `;` separates.
                if i + 1 < chars.len() && chars[i + 1] == ';' {
                    tokens.push(Spanned {
                        token: Token::DSemi,
                        offset,
                    });
                    i += 2;
                } else {
                    tokens.push(Spanned {
                        token: Token::Semicolon,
                        offset,
                    });
                    i += 1;
                }
            }
            '#' => {
                // Comment: skip to end of line
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\'' => {
                // Single-quoted: everything literal until closing '. The word
                // may continue past the closing quote (e.g. `'a'b`).
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
                let s = extend_word(s, &chars, &byte_offsets, &mut i, &mut tokens)?;
                tokens.push(Spanned {
                    token: Token::Word(s),
                    offset,
                });
            }
            '"' => {
                // Double-quoted: allow backslash escapes, defer $ expansion.
                // A word may continue past the closing quote (e.g. `"a"b`), so
                // hand off to extend_word after reading the quoted run.
                i += 1;
                let start = i;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '"' {
                    // Substitution inside double quotes → marked for no-split.
                    if let Some(res) = try_consume_subst(&chars, &mut i, &byte_offsets) {
                        s.push(QUOTED_SUBST_MARK);
                        s.push_str(&res?);
                        continue;
                    }
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
                let s = extend_word(s, &chars, &byte_offsets, &mut i, &mut tokens)?;
                tokens.push(Spanned {
                    token: Token::Word(s),
                    offset,
                });
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
                    s = extend_word(s, &chars, &byte_offsets, &mut i, &mut tokens)?;
                    tokens.push(Spanned {
                        token: Token::Word(s),
                        offset,
                    });
                } else {
                    return Err(LexError {
                        message: "trailing backslash".to_string(),
                        offset,
                    });
                }
            }
            // `2>` and `2>>` stderr redirects (only when 2 is immediately followed by >)
            '2' if i + 1 < chars.len() && chars[i + 1] == '>' => {
                if i + 2 < chars.len() && chars[i + 2] == '>' {
                    tokens.push(Spanned {
                        token: Token::Redirect2Append,
                        offset,
                    });
                    i += 3;
                } else {
                    tokens.push(Spanned {
                        token: Token::Redirect2Out,
                        offset,
                    });
                    i += 2;
                }
            }
            _ => {
                // Bare word. Start extend_word at `i` (not i+1) so a
                // word-initial substitution span (`$(...)`, backtick) is
                // detected by extend_word's try_consume_subst rather than
                // being split apart.
                let s = extend_word(String::new(), &chars, &byte_offsets, &mut i, &mut tokens)?;
                tokens.push(Spanned {
                    token: Token::Word(s),
                    offset,
                });
            }
        }
    }

    // A here-doc whose body never arrived (no terminating newline yet) leaves
    // the command unterminated → the REPL keeps reading.
    if !pending.is_empty() {
        return Err(LexError {
            message: "unterminated here-document".to_string(),
            offset: input.len(),
        });
    }

    tokens.push(Spanned {
        token: Token::Eof,
        offset: input.len(),
    });
    Ok(tokens)
}

/// Read a here-document delimiter after `<<`/`<<-`. A quoted delimiter
/// (`<<'EOF'` or `<<"EOF"`) suppresses body expansion; a bare delimiter ends at
/// whitespace or an operator character. Returns `(delimiter, expand)`.
fn read_heredoc_delim(chars: &[char], i: &mut usize) -> Result<(String, bool), LexError> {
    if *i >= chars.len() {
        return Err(LexError {
            message: "unterminated here-document (missing delimiter)".to_string(),
            offset: 0,
        });
    }
    let quote = chars[*i];
    if quote == '\'' || quote == '"' {
        *i += 1;
        let mut s = String::new();
        while *i < chars.len() && chars[*i] != quote {
            s.push(chars[*i]);
            *i += 1;
        }
        if *i >= chars.len() {
            return Err(LexError {
                message: "unterminated here-document delimiter".to_string(),
                offset: 0,
            });
        }
        *i += 1; // closing quote
        Ok((s, false))
    } else {
        let mut s = String::new();
        while *i < chars.len()
            && !matches!(
                chars[*i],
                ' ' | '\t' | '\n' | '|' | '&' | '<' | '>' | ';' | '(' | ')'
            )
        {
            s.push(chars[*i]);
            *i += 1;
        }
        Ok((s, true))
    }
}

/// Collect a here-document body starting at `*i`, consuming lines until one
/// equals `delim` (after tab-stripping when `strip`). The delimiter line and
/// each body line's trailing newline are consumed; `*i` ends past the
/// delimiter line. An end of input before the delimiter → unterminated.
fn collect_heredoc_body(
    chars: &[char],
    i: &mut usize,
    delim: &str,
    strip: bool,
) -> Result<String, LexError> {
    let mut body = String::new();
    loop {
        let line_start = *i;
        let mut j = line_start;
        while j < chars.len() && chars[j] != '\n' {
            j += 1;
        }
        let raw: String = chars[line_start..j].iter().collect();
        let line: &str = if strip {
            raw.trim_start_matches('\t')
        } else {
            &raw
        };
        if line == delim {
            *i = if j < chars.len() { j + 1 } else { j };
            return Ok(body);
        }
        if j >= chars.len() {
            return Err(LexError {
                message: "unterminated here-document".to_string(),
                offset: 0,
            });
        }
        body.push_str(line);
        body.push('\n');
        *i = j + 1;
    }
}

/// If `chars[*i]` begins a substitution span — `$(...)`, `$((...))`, or a
/// backtick `` `...` `` — consume the whole balanced span and return its
/// verbatim source text (delimiters included), advancing `*i` past it.
/// Returns `None` if no span starts at `*i` (e.g. `$VAR`, `$?`, plain `$`).
fn try_consume_subst(
    chars: &[char],
    i: &mut usize,
    byte_offsets: &[usize],
) -> Option<Result<String, LexError>> {
    if chars[*i] == '`' {
        return Some(consume_backtick(chars, i, byte_offsets));
    }
    if chars[*i] == '$' && *i + 1 < chars.len() && chars[*i + 1] == '(' {
        return Some(consume_paren_span(chars, i, byte_offsets));
    }
    None
}

/// Consume a `$( ... )` or `$(( ... ))` span by paren-depth counting. Quotes
/// inside the span are skipped so a `)` within `'...'`/`"..."` doesn't close it.
/// `$((` arithmetic and `$(` command substitution are handled uniformly: depth
/// returns to 0 on the matching close (`))` for arithmetic, `)` for command
/// substitution). The returned string includes the leading `$(`/`$((`.
fn consume_paren_span(
    chars: &[char],
    i: &mut usize,
    byte_offsets: &[usize],
) -> Result<String, LexError> {
    let start = *i;
    let mut s = String::new();
    s.push('$');
    *i += 1; // now at the first '('
    let mut depth = 0usize;
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '(' => {
                depth += 1;
                s.push(c);
                *i += 1;
            }
            ')' => {
                depth -= 1;
                s.push(c);
                *i += 1;
                if depth == 0 {
                    return Ok(s);
                }
            }
            '\'' => {
                // Single-quoted literal inside the span: copy verbatim.
                s.push(c);
                *i += 1;
                while *i < chars.len() && chars[*i] != '\'' {
                    s.push(chars[*i]);
                    *i += 1;
                }
                if *i < chars.len() {
                    s.push('\'');
                    *i += 1;
                }
            }
            '"' => {
                // Double-quoted run inside the span: copy verbatim, honoring \".
                s.push(c);
                *i += 1;
                while *i < chars.len() && chars[*i] != '"' {
                    if chars[*i] == '\\' && *i + 1 < chars.len() {
                        s.push(chars[*i]);
                        *i += 1;
                    }
                    s.push(chars[*i]);
                    *i += 1;
                }
                if *i < chars.len() {
                    s.push('"');
                    *i += 1;
                }
            }
            '`' => {
                // Nested backtick inside the span: copy to its close verbatim.
                s.push(c);
                *i += 1;
                while *i < chars.len() && chars[*i] != '`' {
                    if chars[*i] == '\\' && *i + 1 < chars.len() {
                        s.push(chars[*i]);
                        *i += 1;
                    }
                    s.push(chars[*i]);
                    *i += 1;
                }
                if *i < chars.len() {
                    s.push('`');
                    *i += 1;
                }
            }
            _ => {
                s.push(c);
                *i += 1;
            }
        }
    }
    Err(LexError {
        message: "unterminated command substitution".to_string(),
        offset: byte_offsets[start],
    })
}

/// Consume a backtick span `` `...` `` (no nesting; `` \` `` escapes a literal
/// backtick). Returns the verbatim source including both backticks.
fn consume_backtick(
    chars: &[char],
    i: &mut usize,
    byte_offsets: &[usize],
) -> Result<String, LexError> {
    let start = *i;
    let mut s = String::new();
    s.push('`');
    *i += 1;
    while *i < chars.len() {
        let c = chars[*i];
        if c == '\\' && *i + 1 < chars.len() {
            s.push(c);
            *i += 1;
            s.push(chars[*i]);
            *i += 1;
        } else if c == '`' {
            s.push('`');
            *i += 1;
            return Ok(s);
        } else {
            s.push(c);
            *i += 1;
        }
    }
    Err(LexError {
        message: "unterminated backtick substitution".to_string(),
        offset: byte_offsets[start],
    })
}

/// Continue accumulating characters into a word, handling embedded quotes,
/// escapes, and substitution spans. Stops at unquoted whitespace or operator
/// characters.
fn extend_word(
    mut s: String,
    chars: &[char],
    byte_offsets: &[usize],
    i: &mut usize,
    _tokens: &mut Vec<Spanned>,
) -> Result<String, LexError> {
    while *i < chars.len() {
        // Substitution spans bind tighter than the word's break characters, so
        // an inner `|`/`;`/space inside `$(...)` does not end the word.
        if let Some(res) = try_consume_subst(chars, i, byte_offsets) {
            s.push_str(&res?);
            continue;
        }
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
                    // A substitution inside double quotes is marked so the
                    // expander knows its output must not be word-split.
                    if let Some(res) = try_consume_subst(chars, i, byte_offsets) {
                        s.push(QUOTED_SUBST_MARK);
                        s.push_str(&res?);
                        continue;
                    }
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
    }
    Ok(s)
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
            vec![
                Token::Word("a".into()),
                Token::Pipe,
                Token::Word("b".into()),
                Token::Eof
            ]
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
            vec![
                Token::Word("a".into()),
                Token::Semicolon,
                Token::Word("b".into()),
                Token::Eof
            ]
        );
    }

    #[test]
    fn single_quoted() {
        assert_eq!(
            tokens("'hello world'"),
            vec![Token::Word("hello world".into()), Token::Eof]
        );
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
        assert_eq!(
            tokens(r#""a\"b""#),
            vec![Token::Word(r#"a"b"#.into()), Token::Eof]
        );
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
        assert_eq!(
            tokens("echo hi # ignored"),
            vec![
                Token::Word("echo".into()),
                Token::Word("hi".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn newline_token() {
        assert_eq!(
            tokens("a\nb"),
            vec![
                Token::Word("a".into()),
                Token::Newline,
                Token::Word("b".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn full_pipeline_example() {
        let toks = tokens(r#"echo "hello world" | grep h > out.txt && cat out.txt"#);
        assert_eq!(
            toks,
            vec![
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
            ]
        );
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

    // ── Tier 4 / Phase A: substitution-span lexing ───────────────────────

    #[test]
    fn lex_keeps_command_substitution_as_one_word() {
        assert_eq!(
            tokens("echo $(echo hi)"),
            vec![
                Token::Word("echo".into()),
                Token::Word("$(echo hi)".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lex_cmdsub_with_inner_spaces_and_pipes() {
        // The inner `|` and spaces must not break the word.
        assert_eq!(
            tokens("$(ls | wc -l)"),
            vec![Token::Word("$(ls | wc -l)".into()), Token::Eof]
        );
    }

    #[test]
    fn lex_nested_cmdsub() {
        assert_eq!(
            tokens("$(echo $(echo x))"),
            vec![Token::Word("$(echo $(echo x))".into()), Token::Eof]
        );
    }

    #[test]
    fn lex_cmdsub_paren_inside_quotes_does_not_close() {
        // The `)` inside the string literal must not end the span.
        assert_eq!(
            tokens(r#"$(echo ")")"#),
            vec![Token::Word(r#"$(echo ")")"#.into()), Token::Eof]
        );
    }

    #[test]
    fn lex_backticks() {
        assert_eq!(
            tokens("`date`"),
            vec![Token::Word("`date`".into()), Token::Eof]
        );
    }

    #[test]
    fn lex_arith() {
        // `$((` is consumed as one balanced span, not confused with `$(`.
        assert_eq!(
            tokens("$((1 + 2))"),
            vec![Token::Word("$((1 + 2))".into()), Token::Eof]
        );
        assert_eq!(
            tokens("$(( (1 + 2) * 3 ))"),
            vec![Token::Word("$(( (1 + 2) * 3 ))".into()), Token::Eof]
        );
    }

    #[test]
    fn lex_cmdsub_in_argument_position() {
        // Adjacent literals join the substitution into one word.
        assert_eq!(
            tokens("pre$(echo X)post"),
            vec![Token::Word("pre$(echo X)post".into()), Token::Eof]
        );
    }

    #[test]
    fn lex_unquoted_cmdsub_has_no_marker() {
        // Unquoted spans carry no quoting marker.
        let toks = tokens("$(echo a b)");
        assert_eq!(toks, vec![Token::Word("$(echo a b)".into()), Token::Eof]);
        if let Token::Word(w) = &toks[0] {
            assert!(!w.contains(QUOTED_SUBST_MARK), "unexpected marker: {:?}", w);
        }
    }

    #[test]
    fn lex_double_quoted_cmdsub_is_marked() {
        // Inside double quotes the span is prefixed with the quoting marker so
        // the expander knows not to word-split its output.
        let toks = tokens(r#""$(echo a b)""#);
        assert_eq!(toks.len(), 2); // Word + Eof
        if let Token::Word(w) = &toks[0] {
            assert!(
                w.starts_with(QUOTED_SUBST_MARK),
                "expected leading marker, got: {:?}",
                w
            );
            assert!(w.contains("$(echo a b)"), "span not intact: {:?}", w);
        } else {
            panic!("expected a Word token, got {:?}", toks[0]);
        }
    }

    #[test]
    fn lex_word_continues_after_closing_quote() {
        // `"a"b` and `'a'b` join into a single word.
        assert_eq!(
            tokens(r#""a"b"#),
            vec![Token::Word("ab".into()), Token::Eof]
        );
        assert_eq!(tokens("'a'b"), vec![Token::Word("ab".into()), Token::Eof]);
    }

    #[test]
    fn lex_unterminated_cmdsub_errors() {
        assert!(lex("echo $(echo hi").is_err());
    }

    #[test]
    fn lex_unterminated_arith_errors() {
        assert!(lex("echo $((1 + 2)").is_err());
    }

    #[test]
    fn lex_unterminated_backtick_errors() {
        assert!(lex("echo `date").is_err());
    }

    #[test]
    fn lex_double_semicolon() {
        assert_eq!(
            tokens("a;; b"),
            vec![
                Token::Word("a".into()),
                Token::DSemi,
                Token::Word("b".into()),
                Token::Eof,
            ]
        );
        // A single `;` is still a Semicolon.
        assert_eq!(
            tokens("a; b"),
            vec![
                Token::Word("a".into()),
                Token::Semicolon,
                Token::Word("b".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lex_lone_marker_passes_through_literally() {
        // A raw U+0001 not introducing a substitution is an ordinary character;
        // the lexer does not strip or special-case it (the expander only honors
        // it directly before a `$(`/backtick span). This keeps single-quoting
        // an identity for arbitrary content.
        assert_eq!(
            tokens("echo \u{1}hi"),
            vec![
                Token::Word("echo".into()),
                Token::Word("\u{1}hi".into()),
                Token::Eof,
            ]
        );
    }
}
