//! Integer arithmetic for `$((expr))` expansion (Tier 4 Phase D).
//!
//! A small recursive-descent evaluator over `i64` with C-like precedence:
//! `+ -` (lowest), then `* / %`, then unary `-`/`+`, then primaries
//! (integer literals, variables, parenthesised sub-expressions).
//!
//! Variables resolve through [`Env`]: a bare name (optionally written `$name`)
//! takes the variable's value parsed as `i64`; an undefined or non-numeric
//! value is `0`, matching common shell behavior. Overflow wraps (modular
//! `i64`, like bash). Division/modulo by zero is an error.

use crate::env::Env;

#[derive(Debug, PartialEq, Eq)]
pub enum ArithError {
    DivByZero,
    Syntax(String),
}

impl std::fmt::Display for ArithError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArithError::DivByZero => write!(f, "division by zero"),
            ArithError::Syntax(m) => write!(f, "{}", m),
        }
    }
}

/// Evaluate an arithmetic expression string, returning its `i64` value.
pub fn eval_arith(src: &str, env: &Env) -> Result<i64, ArithError> {
    let tokens = tokenize(src)?;
    let mut p = Parser {
        tokens: &tokens,
        pos: 0,
        env,
    };
    let v = p.expr()?;
    if p.pos != p.tokens.len() {
        return Err(ArithError::Syntax(format!(
            "unexpected token in arithmetic near position {}",
            p.pos
        )));
    }
    Ok(v)
}

#[derive(Debug, PartialEq, Eq)]
enum Tok {
    Num(i64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, ArithError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            // A `$` prefixing a name is tolerated: `$x` is treated as `x`.
            '$' => i += 1,
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '%' => {
                out.push(Tok::Percent);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            d if d.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let n = s.parse::<i64>().map_err(|_| {
                    ArithError::Syntax(format!("integer literal out of range: {}", s))
                })?;
                out.push(Tok::Num(n));
            }
            a if a.is_alphabetic() || a == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            other => {
                return Err(ArithError::Syntax(format!(
                    "unexpected character '{}' in arithmetic",
                    other
                )))
            }
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [Tok],
    pos: usize,
    env: &'a Env,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    /// expr := term (('+' | '-') term)*
    fn expr(&mut self) -> Result<i64, ArithError> {
        let mut acc = self.term()?;
        loop {
            match self.peek() {
                Some(Tok::Plus) => {
                    self.pos += 1;
                    acc = acc.wrapping_add(self.term()?);
                }
                Some(Tok::Minus) => {
                    self.pos += 1;
                    acc = acc.wrapping_sub(self.term()?);
                }
                _ => break,
            }
        }
        Ok(acc)
    }

    /// term := unary (('*' | '/' | '%') unary)*
    fn term(&mut self) -> Result<i64, ArithError> {
        let mut acc = self.unary()?;
        loop {
            match self.peek() {
                Some(Tok::Star) => {
                    self.pos += 1;
                    acc = acc.wrapping_mul(self.unary()?);
                }
                Some(Tok::Slash) => {
                    self.pos += 1;
                    let rhs = self.unary()?;
                    if rhs == 0 {
                        return Err(ArithError::DivByZero);
                    }
                    acc = acc.wrapping_div(rhs);
                }
                Some(Tok::Percent) => {
                    self.pos += 1;
                    let rhs = self.unary()?;
                    if rhs == 0 {
                        return Err(ArithError::DivByZero);
                    }
                    acc = acc.wrapping_rem(rhs);
                }
                _ => break,
            }
        }
        Ok(acc)
    }

    /// unary := ('-' | '+') unary | primary
    fn unary(&mut self) -> Result<i64, ArithError> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.pos += 1;
                Ok(self.unary()?.wrapping_neg())
            }
            Some(Tok::Plus) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    /// primary := number | ident | '(' expr ')'
    fn primary(&mut self) -> Result<i64, ArithError> {
        match self.tokens.get(self.pos) {
            Some(Tok::Num(n)) => {
                let n = *n;
                self.pos += 1;
                Ok(n)
            }
            Some(Tok::Ident(name)) => {
                let name = name.clone();
                self.pos += 1;
                Ok(self
                    .env
                    .get(&name)
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(0))
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let v = self.expr()?;
                match self.tokens.get(self.pos) {
                    Some(Tok::RParen) => {
                        self.pos += 1;
                        Ok(v)
                    }
                    _ => Err(ArithError::Syntax("expected ')'".to_string())),
                }
            }
            Some(t) => Err(ArithError::Syntax(format!("unexpected token {:?}", t))),
            None => Err(ArithError::Syntax(
                "unexpected end of expression".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        let mut e = Env::new();
        e.set("N", "5");
        e.set("WORD", "hello");
        e
    }

    fn ev(s: &str) -> Result<i64, ArithError> {
        eval_arith(s, &env())
    }

    #[test]
    fn precedence() {
        assert_eq!(ev("1 + 2 * 3").unwrap(), 7);
    }

    #[test]
    fn parens() {
        assert_eq!(ev("(1 + 2) * 3").unwrap(), 9);
    }

    #[test]
    fn subtraction_and_unary() {
        assert_eq!(ev("10 - 3 - 2").unwrap(), 5);
        assert_eq!(ev("-5 + 8").unwrap(), 3);
        assert_eq!(ev("-(2 + 3)").unwrap(), -5);
    }

    #[test]
    fn mul_div_mod() {
        assert_eq!(ev("20 / 4").unwrap(), 5);
        assert_eq!(ev("17 % 5").unwrap(), 2);
    }

    #[test]
    fn variables() {
        assert_eq!(ev("N * 2").unwrap(), 10);
        assert_eq!(ev("$N + 1").unwrap(), 6); // `$`-prefix tolerated
    }

    #[test]
    fn undefined_is_zero() {
        assert_eq!(ev("UNDEF + 4").unwrap(), 4);
    }

    #[test]
    fn non_numeric_is_zero() {
        assert_eq!(ev("WORD + 4").unwrap(), 4);
    }

    #[test]
    fn div_by_zero_errors() {
        assert_eq!(ev("1 / 0"), Err(ArithError::DivByZero));
        assert_eq!(ev("1 % 0"), Err(ArithError::DivByZero));
    }

    #[test]
    fn syntax_error() {
        assert!(matches!(ev("1 +"), Err(ArithError::Syntax(_))));
        assert!(matches!(ev("1 2"), Err(ArithError::Syntax(_))));
        assert!(matches!(ev("@"), Err(ArithError::Syntax(_))));
    }

    #[test]
    fn overflow_wraps_not_panics() {
        // Must not panic in debug builds.
        let _ = ev("9223372036854775807 + 1");
    }
}
