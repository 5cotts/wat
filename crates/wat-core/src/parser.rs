use crate::ast::{Command, CompoundCommand, List, Pipeline, Redirect, Separator, SimpleCommand};
use crate::lexer::{lex, LexError, Spanned, Token};

/// A parse error with a byte offset and human-readable message. `incomplete`
/// is true when the parser reached end-of-input while still expecting more
/// (an open construct or unterminated quote/substitution) — the REPL uses it
/// to keep reading continuation lines rather than report an error.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
    pub incomplete: bool,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error at offset {}: {}", self.offset, self.message)
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        // Unterminated quotes / substitutions mean "need more input".
        let incomplete = e.message.contains("unterminated");
        ParseError {
            message: e.message,
            offset: e.offset,
            incomplete,
        }
    }
}

/// Parse a shell input string into a [`List`] of pipelines.
pub fn parse(input: &str) -> Result<List, ParseError> {
    let tokens = lex(input)?;
    let mut p = Parser { tokens, pos: 0 };
    p.parse_list()
}

struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn offset(&self) -> usize {
        self.tokens[self.pos].offset
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos].token;
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Token::Newline {
            self.advance();
        }
    }

    fn parse_list(&mut self) -> Result<List, ParseError> {
        self.parse_list_until(&[])
    }

    /// True if the next token is one of `stops` in command position (a bare
    /// keyword like `then`/`fi`/`done` terminating the current compound body).
    fn at_stop_word(&self, stops: &[&str]) -> bool {
        matches!(self.peek(), Token::Word(w) if stops.contains(&w.as_str()))
    }

    fn at_keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Token::Word(w) if w == kw)
    }

    /// Consume an expected keyword. EOF → `incomplete` (the REPL keeps reading);
    /// any other token → a hard syntax error.
    fn expect_keyword(&mut self, kw: &str) -> Result<(), ParseError> {
        if self.at_keyword(kw) {
            self.advance();
            Ok(())
        } else if *self.peek() == Token::Eof {
            Err(ParseError {
                message: format!("expected '{}'", kw),
                offset: self.offset(),
                incomplete: true,
            })
        } else {
            Err(ParseError {
                message: format!("expected '{}', found '{}'", kw, self.peek().display()),
                offset: self.offset(),
                incomplete: false,
            })
        }
    }

    /// Parse a list of pipelines, stopping (without consuming) at EOF or at a
    /// `stops` keyword in command position. Top-level parsing passes no stops.
    fn parse_list_until(&mut self, stops: &[&str]) -> Result<List, ParseError> {
        let mut items = Vec::new();

        self.skip_newlines();

        while *self.peek() != Token::Eof && !self.at_stop_word(stops) {
            let pipeline = self.parse_pipeline()?;
            let sep = match self.peek() {
                Token::Semicolon => {
                    self.advance();
                    self.skip_newlines();
                    Separator::Semi
                }
                Token::And => {
                    self.advance();
                    self.skip_newlines();
                    Separator::And
                }
                Token::Or => {
                    self.advance();
                    self.skip_newlines();
                    Separator::Or
                }
                Token::Background => {
                    self.advance();
                    self.skip_newlines();
                    Separator::Background
                }
                Token::Newline => {
                    self.advance();
                    self.skip_newlines();
                    Separator::Semi
                }
                Token::Eof => Separator::End,
                other => {
                    return Err(ParseError {
                        message: format!("unexpected token '{}'", other.display()),
                        offset: self.offset(),
                        incomplete: false,
                    })
                }
            };
            let is_end = sep == Separator::End;
            items.push((pipeline, sep));
            if is_end {
                break;
            }
        }

        Ok(List(items))
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let mut commands = Vec::new();
        commands.push(self.parse_command()?);

        while *self.peek() == Token::Pipe {
            self.advance();
            self.skip_newlines();
            commands.push(self.parse_command()?);
        }

        Ok(Pipeline(commands))
    }

    /// Parse one command: a compound command if a control-flow keyword is in
    /// command position, otherwise a simple command.
    fn parse_command(&mut self) -> Result<Command, ParseError> {
        if let Token::Word(w) = self.peek() {
            match w.as_str() {
                "if" => return Ok(Command::Compound(self.parse_if()?)),
                // A terminator keyword in command position with no open
                // construct is a syntax error (we'd otherwise treat it as a
                // command name and fail with "command not found").
                "then" | "elif" | "else" | "fi" => {
                    return Err(ParseError {
                        message: format!("unexpected '{}'", w),
                        offset: self.offset(),
                        incomplete: false,
                    });
                }
                _ => {}
            }
        }
        Ok(Command::Simple(self.parse_simple_command()?))
    }

    /// `if cond; then body; [elif cond; then body;]* [else body;] fi`.
    fn parse_if(&mut self) -> Result<CompoundCommand, ParseError> {
        self.advance(); // consume `if`
        let mut branches = Vec::new();

        // `if` and each `elif` introduce a (condition, then-body) pair.
        loop {
            let cond = self.parse_list_until(&["then"])?;
            self.expect_keyword("then")?;
            let body = self.parse_list_until(&["elif", "else", "fi"])?;
            branches.push((cond, body));
            if !self.at_keyword("elif") {
                break;
            }
            self.advance(); // consume `elif`
        }

        let else_body = if self.at_keyword("else") {
            self.advance();
            Some(self.parse_list_until(&["fi"])?)
        } else {
            None
        };

        self.expect_keyword("fi")?;
        Ok(CompoundCommand::If {
            branches,
            else_body,
        })
    }

    fn parse_simple_command(&mut self) -> Result<SimpleCommand, ParseError> {
        // Collect words and redirects; first word is the command name.
        let mut words: Vec<String> = Vec::new();
        let mut redirects: Vec<Redirect> = Vec::new();

        loop {
            match self.peek().clone() {
                Token::Word(w) => {
                    self.advance();
                    words.push(w);
                }
                Token::RedirectOut => {
                    self.advance();
                    let target = self.expect_word("expected filename after '>'")?;
                    redirects.push(Redirect::Out(target));
                }
                Token::RedirectAppend => {
                    self.advance();
                    let target = self.expect_word("expected filename after '>>'")?;
                    redirects.push(Redirect::Append(target));
                }
                Token::RedirectIn => {
                    self.advance();
                    let target = self.expect_word("expected filename after '<'")?;
                    redirects.push(Redirect::In(target));
                }
                Token::Redirect2Out => {
                    self.advance();
                    let target = self.expect_word("expected filename after '2>'")?;
                    redirects.push(Redirect::Err(target));
                }
                Token::Redirect2Append => {
                    self.advance();
                    let target = self.expect_word("expected filename after '2>>'")?;
                    redirects.push(Redirect::Err(target)); // append not tracked separately yet
                }
                _ => break,
            }
        }

        if words.is_empty() {
            return Err(ParseError {
                message: "expected a command".to_string(),
                offset: self.offset(),
                incomplete: false,
            });
        }

        // Peel off leading `NAME=value` assignment words. They must precede the
        // command name; once a non-assignment word appears, the rest are
        // arguments even if they look like `a=b`.
        let mut assignments: Vec<(String, String)> = Vec::new();
        let mut idx = 0;
        while idx < words.len() {
            match split_assignment(&words[idx]) {
                Some(kv) => {
                    assignments.push(kv);
                    idx += 1;
                }
                None => break,
            }
        }
        let rest = words.split_off(idx);
        // `rest` is the command name + args; empty means a pure assignment.
        let mut rest = rest.into_iter();
        let name = rest.next().unwrap_or_default();
        let args: Vec<String> = rest.collect();

        Ok(SimpleCommand {
            assignments,
            name,
            args,
            redirects,
        })
    }

    fn expect_word(&mut self, msg: &str) -> Result<String, ParseError> {
        let offset = self.offset();
        match self.peek().clone() {
            Token::Word(w) => {
                self.advance();
                Ok(w)
            }
            _ => Err(ParseError {
                message: msg.to_string(),
                offset,
                incomplete: false,
            }),
        }
    }
}

/// If `word` is an assignment of the form `NAME=value` — where `NAME` is a
/// valid shell identifier (`[A-Za-z_][A-Za-z0-9_]*`) — return `(NAME, value)`
/// with the (still-unexpanded) value. The value may be empty or contain further
/// `=` characters. Returns `None` otherwise.
fn split_assignment(word: &str) -> Option<(String, String)> {
    let eq = word.find('=')?;
    let name = &word[..eq];
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), word[eq + 1..].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    fn simple(c: &Command) -> &SimpleCommand {
        match c {
            Command::Simple(sc) => sc,
            Command::Compound(_) => panic!("expected a simple command"),
        }
    }

    fn only_command(list: &List) -> &Command {
        &list.0[0].0 .0[0]
    }

    #[test]
    fn parse_if_structure() {
        let list = parse("if true; then echo a; else echo b; fi").unwrap();
        match only_command(&list) {
            Command::Compound(CompoundCommand::If {
                branches,
                else_body,
            }) => {
                assert_eq!(branches.len(), 1);
                assert!(else_body.is_some());
            }
            other => panic!("expected if, got {:?}", other),
        }
    }

    #[test]
    fn parse_if_elif_branches() {
        let list = parse("if a; then x; elif b; then y; elif c; then z; fi").unwrap();
        match only_command(&list) {
            Command::Compound(CompoundCommand::If { branches, .. }) => {
                assert_eq!(branches.len(), 3); // if + 2 elif
            }
            other => panic!("expected if, got {:?}", other),
        }
    }

    #[test]
    fn unterminated_if_is_incomplete() {
        let err = parse("if true; then echo x").unwrap_err();
        assert!(err.incomplete, "expected incomplete, got: {:?}", err);
    }

    #[test]
    fn missing_then_is_incomplete_at_eof() {
        let err = parse("if true").unwrap_err();
        assert!(err.incomplete, "expected incomplete, got: {:?}", err);
    }

    #[test]
    fn stray_fi_is_hard_error() {
        let err = parse("fi").unwrap_err();
        assert!(
            !err.incomplete,
            "stray fi should be a hard error: {:?}",
            err
        );
    }

    fn cmd(name: &str, args: &[&str]) -> Command {
        Command::Simple(SimpleCommand {
            assignments: vec![],
            name: name.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            redirects: vec![],
        })
    }

    fn cmd_r(name: &str, args: &[&str], redirects: Vec<Redirect>) -> Command {
        Command::Simple(SimpleCommand {
            assignments: vec![],
            name: name.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            redirects,
        })
    }

    #[test]
    fn simple_command() {
        let list = parse("echo hello").unwrap();
        assert_eq!(
            list.0,
            vec![(Pipeline(vec![cmd("echo", &["hello"])]), Separator::End)]
        );
    }

    #[test]
    fn assignment_prefix_before_command() {
        let list = parse("x=5 echo hi").unwrap();
        let c = simple(&list.0[0].0 .0[0]);
        assert_eq!(c.assignments, vec![("x".to_string(), "5".to_string())]);
        assert_eq!(c.name, "echo");
        assert_eq!(c.args, vec!["hi".to_string()]);
    }

    #[test]
    fn pure_assignment_has_empty_name() {
        let list = parse("foo=bar").unwrap();
        let c = simple(&list.0[0].0 .0[0]);
        assert_eq!(c.assignments, vec![("foo".to_string(), "bar".to_string())]);
        assert_eq!(c.name, "");
        assert!(c.args.is_empty());
    }

    #[test]
    fn multiple_assignments() {
        let list = parse("a=1 b=2 cmd").unwrap();
        let c = simple(&list.0[0].0 .0[0]);
        assert_eq!(
            c.assignments,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ]
        );
        assert_eq!(c.name, "cmd");
    }

    #[test]
    fn assignment_looking_arg_after_name_is_arg() {
        // `x=5` after the command name is a normal argument, not an assignment.
        let list = parse("echo x=5").unwrap();
        let c = simple(&list.0[0].0 .0[0]);
        assert!(c.assignments.is_empty());
        assert_eq!(c.name, "echo");
        assert_eq!(c.args, vec!["x=5".to_string()]);
    }

    #[test]
    fn value_with_equals_and_empty() {
        let list = parse("PATH=/a:/b=c x=").unwrap();
        let c = simple(&list.0[0].0 .0[0]);
        assert_eq!(
            c.assignments,
            vec![
                ("PATH".to_string(), "/a:/b=c".to_string()),
                ("x".to_string(), String::new())
            ]
        );
        assert_eq!(c.name, "");
    }

    #[test]
    fn non_identifier_is_not_assignment() {
        // Leading `$` makes it not a NAME=value assignment word → it's the name.
        let list = parse("1abc=5").unwrap();
        let c = simple(&list.0[0].0 .0[0]);
        assert!(c.assignments.is_empty());
        assert_eq!(c.name, "1abc=5");
    }

    #[test]
    fn pipeline() {
        let list = parse("ls | grep foo").unwrap();
        assert_eq!(
            list.0,
            vec![(
                Pipeline(vec![cmd("ls", &[]), cmd("grep", &["foo"])]),
                Separator::End
            )]
        );
    }

    #[test]
    fn semicolon_list() {
        let list = parse("a ; b").unwrap();
        assert_eq!(list.0.len(), 2);
        assert_eq!(list.0[0].1, Separator::Semi);
    }

    #[test]
    fn and_list() {
        let list = parse("a && b").unwrap();
        assert_eq!(list.0[0].1, Separator::And);
    }

    #[test]
    fn or_list() {
        let list = parse("a || b").unwrap();
        assert_eq!(list.0[0].1, Separator::Or);
    }

    #[test]
    fn redirect_out() {
        let list = parse("echo hi > out.txt").unwrap();
        let cmd = simple(&list.0[0].0 .0[0]);
        assert_eq!(cmd.redirects, vec![Redirect::Out("out.txt".into())]);
    }

    #[test]
    fn redirect_append() {
        let list = parse("echo hi >> out.txt").unwrap();
        let cmd = simple(&list.0[0].0 .0[0]);
        assert_eq!(cmd.redirects, vec![Redirect::Append("out.txt".into())]);
    }

    #[test]
    fn redirect_in() {
        let list = parse("cat < in.txt").unwrap();
        let cmd = simple(&list.0[0].0 .0[0]);
        assert_eq!(cmd.redirects, vec![Redirect::In("in.txt".into())]);
    }

    #[test]
    fn full_acceptance_example() {
        let list = parse(r#"echo "hello world" | grep h > out.txt && cat out.txt"#).unwrap();
        assert_eq!(list.0.len(), 2);

        let (pipe0, sep0) = &list.0[0];
        assert_eq!(sep0, &Separator::And);
        assert_eq!(pipe0.0[0], cmd("echo", &["hello world"]));
        assert_eq!(
            pipe0.0[1],
            cmd_r("grep", &["h"], vec![Redirect::Out("out.txt".into())])
        );

        let (pipe1, sep1) = &list.0[1];
        assert_eq!(sep1, &Separator::End);
        assert_eq!(pipe1.0[0], cmd("cat", &["out.txt"]));
    }

    #[test]
    fn empty_input_gives_empty_list() {
        let list = parse("").unwrap();
        assert!(list.0.is_empty());
    }

    #[test]
    fn error_has_offset() {
        let err = parse(">").unwrap_err();
        assert!(!err.message.is_empty());
        assert_eq!(err.offset, 1); // after '>'
    }

    #[test]
    fn lex_error_propagates() {
        let err = parse("'unterminated").unwrap_err();
        assert!(err.message.contains("single quote"));
    }

    #[test]
    fn multiple_redirects() {
        let list = parse("cmd < in.txt > out.txt").unwrap();
        let cmd = simple(&list.0[0].0 .0[0]);
        assert_eq!(
            cmd.redirects,
            vec![
                Redirect::In("in.txt".into()),
                Redirect::Out("out.txt".into()),
            ]
        );
    }

    #[test]
    fn newline_acts_as_separator() {
        let list = parse("a\nb").unwrap();
        assert_eq!(list.0.len(), 2);
    }
}
