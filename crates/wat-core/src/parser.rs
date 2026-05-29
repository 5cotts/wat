use crate::ast::{Command, List, Pipeline, Redirect, Separator};
use crate::lexer::{lex, LexError, Spanned, Token};

/// A parse error with a byte offset and human-readable message.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error at offset {}: {}", self.offset, self.message)
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError { message: e.message, offset: e.offset }
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
        let mut items = Vec::new();

        self.skip_newlines();

        while *self.peek() != Token::Eof {
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

    fn parse_command(&mut self) -> Result<Command, ParseError> {
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
                _ => break,
            }
        }

        if words.is_empty() {
            return Err(ParseError {
                message: "expected a command".to_string(),
                offset: self.offset(),
            });
        }

        let name = words.remove(0);
        Ok(Command { name, args: words, redirects })
    }

    fn expect_word(&mut self, msg: &str) -> Result<String, ParseError> {
        let offset = self.offset();
        match self.peek().clone() {
            Token::Word(w) => {
                self.advance();
                Ok(w)
            }
            _ => Err(ParseError { message: msg.to_string(), offset }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    fn cmd(name: &str, args: &[&str]) -> Command {
        Command {
            name: name.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            redirects: vec![],
        }
    }

    fn cmd_r(name: &str, args: &[&str], redirects: Vec<Redirect>) -> Command {
        Command {
            name: name.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            redirects,
        }
    }

    #[test]
    fn simple_command() {
        let list = parse("echo hello").unwrap();
        assert_eq!(list.0, vec![(Pipeline(vec![cmd("echo", &["hello"])]), Separator::End)]);
    }

    #[test]
    fn pipeline() {
        let list = parse("ls | grep foo").unwrap();
        assert_eq!(
            list.0,
            vec![(Pipeline(vec![cmd("ls", &[]), cmd("grep", &["foo"])]), Separator::End)]
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
        let cmd = &list.0[0].0 .0[0];
        assert_eq!(cmd.redirects, vec![Redirect::Out("out.txt".into())]);
    }

    #[test]
    fn redirect_append() {
        let list = parse("echo hi >> out.txt").unwrap();
        let cmd = &list.0[0].0 .0[0];
        assert_eq!(cmd.redirects, vec![Redirect::Append("out.txt".into())]);
    }

    #[test]
    fn redirect_in() {
        let list = parse("cat < in.txt").unwrap();
        let cmd = &list.0[0].0 .0[0];
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
        let cmd = &list.0[0].0 .0[0];
        assert_eq!(cmd.redirects, vec![
            Redirect::In("in.txt".into()),
            Redirect::Out("out.txt".into()),
        ]);
    }

    #[test]
    fn newline_acts_as_separator() {
        let list = parse("a\nb").unwrap();
        assert_eq!(list.0.len(), 2);
    }
}
