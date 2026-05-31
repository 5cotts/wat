use crate::context::Context;
use crate::env::Env;
use crate::io::OutputSink;
use crate::lexer::QUOTED_SUBST_MARK;

/// Maximum command-substitution nesting depth before we refuse to recurse.
const MAX_SUBST_DEPTH: u32 = 32;

/// Expand `$VAR`, `${VAR}`, `$?`, and leading `~` in a word. Variable
/// references expand to empty string if undefined. This is the **pure**
/// expander: it cannot run commands, so `$(...)`/backticks are left as-is
/// (the internal quoting marker is dropped). Used where no `Context` is
/// available (e.g. PTY routing checks).
pub fn expand_word(word: &str, env: &Env) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::with_capacity(word.len());
    let mut i = 0;

    // Leading ~ expands to $HOME (only when it's the whole token or followed by /)
    if !chars.is_empty() && chars[0] == '~' && (chars.len() == 1 || chars[1] == '/') {
        out.push_str(env.home());
        i = 1;
    }

    while i < chars.len() {
        if chars[i] == QUOTED_SUBST_MARK {
            // Internal marker is meaningful only to expand_word_ctx; drop it.
            i += 1;
        } else if chars[i] == '$' {
            let (text, next) = expand_dollar(&chars, i, env);
            out.push_str(&text);
            i = next;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

/// Expand a single `$...` occurrence starting at `chars[i] == '$'`. Handles
/// `${VAR}`, `$?`, `$NAME`, and a bare/other `$` (left literal). Returns the
/// expanded text and the index just past what was consumed. Does NOT handle
/// `$(`/`$((` — callers detect substitution spans before reaching here.
fn expand_dollar(chars: &[char], i: usize, env: &Env) -> (String, usize) {
    debug_assert_eq!(chars[i], '$');
    let mut i = i + 1;
    if i >= chars.len() {
        return ("$".to_string(), i);
    }
    if chars[i] == '{' {
        i += 1;
        let start = i;
        while i < chars.len() && chars[i] != '}' {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        if i < chars.len() {
            i += 1; // consume '}'
        }
        (expand_var(&name, env).to_string(), i)
    } else if chars[i] == '?' {
        (env.last_exit_code.to_string(), i + 1)
    } else if chars[i].is_alphabetic() || chars[i] == '_' {
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        (expand_var(&name, env).to_string(), i)
    } else {
        // Bare `$` or `$` before a non-name char: leave the `$` literal and let
        // the caller process the following character normally.
        ("$".to_string(), i)
    }
}

fn expand_var<'a>(name: &str, env: &'a Env) -> &'a str {
    env.get(name).unwrap_or("")
}

/// Kind of a `$`/backtick substitution span.
enum SubstKind {
    /// `$(...)` or `` `...` `` — run as a command, capture stdout.
    Command,
    /// `$((...))` — arithmetic. Evaluated in Phase D; left literal until then.
    Arith,
}

/// If `chars[i]` begins a substitution span, return its kind, inner source
/// (without delimiters), and the index just past the span. Mirrors the
/// lexer's balancing (quote-skipping, nesting) so the boundaries match.
fn extract_subst(chars: &[char], i: usize) -> Option<(SubstKind, String, usize)> {
    if chars[i] == '`' {
        let mut j = i + 1;
        let mut inner = String::new();
        while j < chars.len() {
            if chars[j] == '\\' && j + 1 < chars.len() {
                inner.push(chars[j]);
                inner.push(chars[j + 1]);
                j += 2;
            } else if chars[j] == '`' {
                return Some((SubstKind::Command, inner, j + 1));
            } else {
                inner.push(chars[j]);
                j += 1;
            }
        }
        return None; // unterminated (lexer would have rejected)
    }
    if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '(' {
        let arith = i + 2 < chars.len() && chars[i + 2] == '(';
        let start_inner = if arith { i + 3 } else { i + 2 };
        let mut j = i + 1;
        let mut depth = 0usize;
        while j < chars.len() {
            match chars[j] {
                '(' => {
                    depth += 1;
                    j += 1;
                }
                ')' => {
                    depth -= 1;
                    j += 1;
                    if depth == 0 {
                        // arith inner excludes the trailing `))`; command sub
                        // excludes the single trailing `)`.
                        let end_inner = if arith { j - 2 } else { j - 1 };
                        let inner: String = chars[start_inner..end_inner].iter().collect();
                        let kind = if arith {
                            SubstKind::Arith
                        } else {
                            SubstKind::Command
                        };
                        return Some((kind, inner, j));
                    }
                }
                '\'' => {
                    j += 1;
                    while j < chars.len() && chars[j] != '\'' {
                        j += 1;
                    }
                    if j < chars.len() {
                        j += 1;
                    }
                }
                '"' => {
                    j += 1;
                    while j < chars.len() && chars[j] != '"' {
                        if chars[j] == '\\' && j + 1 < chars.len() {
                            j += 1;
                        }
                        j += 1;
                    }
                    if j < chars.len() {
                        j += 1;
                    }
                }
                _ => j += 1,
            }
        }
        return None; // unterminated
    }
    None
}

/// Full expansion for the command path: `~`, `$VAR`/`${VAR}`, `$?`, command
/// substitution `$(...)`/`` `...` ``, and (Phase D) arithmetic `$((...))`.
/// May run sub-pipelines, so it needs `&mut Context`; substitution stderr is
/// forwarded to `err`.
///
/// Returns the expanded word(s) after field splitting. Output of an **unquoted**
/// command substitution is split on IFS whitespace (space/tab/newline) into
/// separate fields, with adjacent literals joining the first/last field. A
/// **quoted** substitution (preceded by `QUOTED_SUBST_MARK`), literal text, and
/// `$VAR` expansions are never split — they append to the current field. This
/// mirrors POSIX field splitting, restricted to command substitution (this
/// shell does not IFS-split `$VAR`). Globbing is applied by the caller.
pub fn expand_word_ctx(word: &str, ctx: &mut Context, err: &mut dyn OutputSink) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut fields: Vec<String> = Vec::new();
    // `None` = no field in progress; `Some(s)` = a field is being built (s may
    // be empty, e.g. from a quoted empty substitution).
    let mut current: Option<String> = None;
    let mut next_quoted = false;
    let mut i = 0;

    if !chars.is_empty() && chars[0] == '~' && (chars.len() == 1 || chars[1] == '/') {
        push_literal(&mut current, ctx.env.home());
        i = 1;
    }

    while i < chars.len() {
        let c = chars[i];
        if c == QUOTED_SUBST_MARK {
            // The immediately following substitution was double-quoted.
            next_quoted = true;
            i += 1;
            continue;
        }
        // `quoted` applies only to the token handled in this iteration.
        let quoted = next_quoted;
        next_quoted = false;

        if c == '`' || (c == '$' && i + 1 < chars.len() && chars[i + 1] == '(') {
            if let Some((kind, inner, next)) = extract_subst(&chars, i) {
                match kind {
                    SubstKind::Command => {
                        let output = run_command_subst(&inner, ctx, err);
                        if quoted {
                            push_literal(&mut current, &output);
                        } else {
                            push_split(&mut fields, &mut current, &output);
                        }
                    }
                    // Phase D evaluates this; until then leave it literal (and
                    // never split, like a quoted value).
                    SubstKind::Arith => {
                        let lit: String = chars[i..next].iter().collect();
                        push_literal(&mut current, &lit);
                    }
                }
                i = next;
                continue;
            }
            // Not a balanced span (shouldn't happen post-lexer): fall through.
        }
        if c == '$' {
            let (text, next) = expand_dollar(&chars, i, &ctx.env);
            push_literal(&mut current, &text);
            i = next;
        } else {
            push_literal(&mut current, &c.to_string());
            i += 1;
        }
    }

    if let Some(c) = current {
        fields.push(c);
    }
    fields
}

/// Append non-splittable text to the field in progress, starting one if needed.
fn push_literal(current: &mut Option<String>, text: &str) {
    match current {
        Some(s) => s.push_str(text),
        None => *current = Some(text.to_string()),
    }
}

/// Append the output of an unquoted command substitution, splitting on runs of
/// IFS whitespace. Whitespace runs separate fields; text adjacent to a run on
/// its left ends a field and text on its right starts one (so literals join the
/// first/last field). Leading/trailing/repeated whitespace produces no empty
/// fields.
fn push_split(fields: &mut Vec<String>, current: &mut Option<String>, s: &str) {
    let is_ifs = |c: char| c == ' ' || c == '\t' || c == '\n';
    if s.is_empty() {
        return;
    }
    let has_lead = s.starts_with(is_ifs);
    let has_trail = s.ends_with(is_ifs);
    let tokens: Vec<&str> = s.split(is_ifs).filter(|t| !t.is_empty()).collect();

    if tokens.is_empty() {
        // All whitespace: acts purely as a field separator.
        if let Some(c) = current.take() {
            fields.push(c);
        }
        return;
    }
    if has_lead {
        if let Some(c) = current.take() {
            fields.push(c);
        }
    }
    push_literal(current, tokens[0]);
    for t in &tokens[1..] {
        if let Some(c) = current.take() {
            fields.push(c);
        }
        push_literal(current, t);
    }
    if has_trail {
        if let Some(c) = current.take() {
            fields.push(c);
        }
    }
}

/// Execute `inner` as a command list, returning its stdout with all trailing
/// newlines stripped (POSIX command-substitution semantics). Enforces the
/// recursion-depth guard.
fn run_command_subst(inner: &str, ctx: &mut Context, err: &mut dyn OutputSink) -> String {
    if ctx.subst_depth >= MAX_SUBST_DEPTH {
        err.write(b"wat: command substitution nested too deeply\n");
        return String::new();
    }
    ctx.subst_depth += 1;
    let (_code, bytes) = crate::eval::eval_capture_stdout(inner, ctx, err);
    ctx.subst_depth -= 1;
    let mut s = String::from_utf8_lossy(&bytes).into_owned();
    while s.ends_with('\n') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    fn env() -> Env {
        let mut e = Env::new();
        e.set("FOO", "bar");
        e.set("HOME", "/home/5cotts");
        e
    }

    #[test]
    fn plain_word() {
        assert_eq!(expand_word("hello", &env()), "hello");
    }

    #[test]
    fn var_expansion() {
        assert_eq!(expand_word("$FOO", &env()), "bar");
    }

    #[test]
    fn braced_var() {
        assert_eq!(expand_word("${FOO}", &env()), "bar");
    }

    #[test]
    fn undefined_var_empty() {
        assert_eq!(expand_word("$UNDEF", &env()), "");
    }

    #[test]
    fn tilde_expands() {
        assert_eq!(expand_word("~", &env()), "/home/5cotts");
    }

    #[test]
    fn tilde_slash() {
        assert_eq!(expand_word("~/foo", &env()), "/home/5cotts/foo");
    }

    #[test]
    fn dollar_question() {
        let mut e = env();
        e.last_exit_code = 42;
        assert_eq!(expand_word("$?", &e), "42");
    }

    #[test]
    fn mixed_expansion() {
        assert_eq!(expand_word("${FOO}baz", &env()), "barbaz");
    }

    #[test]
    fn dollar_at_end() {
        assert_eq!(expand_word("end$", &env()), "end$");
    }
}
