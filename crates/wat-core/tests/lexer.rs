use insta::assert_debug_snapshot;
use wat_core::lexer::{lex, Token};

fn toks(input: &str) -> Vec<Token> {
    lex(input).unwrap().into_iter().map(|s| s.token).collect()
}

#[test]
fn snap_empty() {
    assert_debug_snapshot!(toks(""));
}

#[test]
fn snap_echo_hello() {
    assert_debug_snapshot!(toks("echo hello"));
}

#[test]
fn snap_pipeline() {
    assert_debug_snapshot!(toks("ls | grep foo"));
}

#[test]
fn snap_redirect_out() {
    assert_debug_snapshot!(toks("echo hi > out.txt"));
}

#[test]
fn snap_redirect_append() {
    assert_debug_snapshot!(toks("echo hi >> out.txt"));
}

#[test]
fn snap_redirect_in() {
    assert_debug_snapshot!(toks("cat < in.txt"));
}

#[test]
fn snap_and_or() {
    assert_debug_snapshot!(toks("a && b || c"));
}

#[test]
fn snap_semicolon() {
    assert_debug_snapshot!(toks("a ; b ; c"));
}

#[test]
fn snap_single_quoted() {
    assert_debug_snapshot!(toks("echo 'hello world'"));
}

#[test]
fn snap_double_quoted() {
    assert_debug_snapshot!(toks(r#"echo "hello world""#));
}

#[test]
fn snap_double_quoted_escape() {
    assert_debug_snapshot!(toks(r#"echo "a\"b""#));
}

#[test]
fn snap_mixed_quoting() {
    assert_debug_snapshot!(toks(r#"hel'lo '"world""#));
}

#[test]
fn snap_backslash_space() {
    assert_debug_snapshot!(toks(r"echo a\ b"));
}

#[test]
fn snap_comment() {
    assert_debug_snapshot!(toks("echo hi # this is a comment"));
}

#[test]
fn snap_newline() {
    assert_debug_snapshot!(toks("a\nb"));
}

#[test]
fn snap_full_acceptance() {
    assert_debug_snapshot!(toks(r#"echo "hello world" | grep h > out.txt && cat out.txt"#));
}

#[test]
fn snap_multiple_args() {
    assert_debug_snapshot!(toks("ls -la /tmp"));
}

#[test]
fn snap_dollar_preserved_in_double_quote() {
    assert_debug_snapshot!(toks(r#"echo "$HOME""#));
}

#[test]
fn snap_single_quote_no_expansion() {
    assert_debug_snapshot!(toks(r#"echo '$HOME'"#));
}

#[test]
fn snap_empty_double_quote() {
    assert_debug_snapshot!(toks(r#"echo """#));
}

#[test]
fn snap_empty_single_quote() {
    assert_debug_snapshot!(toks("echo ''"));
}

#[test]
fn snap_chained_pipes() {
    assert_debug_snapshot!(toks("a | b | c | d"));
}

#[test]
fn snap_redirect_no_space() {
    assert_debug_snapshot!(toks("echo hi>out.txt"));
}

#[test]
fn snap_semicolons_no_space() {
    assert_debug_snapshot!(toks("a;b;c"));
}

#[test]
fn snap_lex_error_single_quote() {
    let err = lex("'unclosed").unwrap_err();
    assert_debug_snapshot!(err);
}

#[test]
fn snap_lex_error_double_quote() {
    let err = lex("\"unclosed").unwrap_err();
    assert_debug_snapshot!(err);
}

#[test]
fn snap_lex_error_trailing_backslash() {
    let err = lex("echo \\").unwrap_err();
    assert_debug_snapshot!(err);
}
