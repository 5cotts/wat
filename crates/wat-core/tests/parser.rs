use insta::assert_debug_snapshot;
use wat_core::parser::parse;

#[test]
fn snap_simple_command() {
    assert_debug_snapshot!(parse("echo hello").unwrap());
}

#[test]
fn snap_pipeline() {
    assert_debug_snapshot!(parse("ls | grep foo").unwrap());
}

#[test]
fn snap_and_list() {
    assert_debug_snapshot!(parse("a && b").unwrap());
}

#[test]
fn snap_or_list() {
    assert_debug_snapshot!(parse("a || b").unwrap());
}

#[test]
fn snap_semicolon_list() {
    assert_debug_snapshot!(parse("a ; b ; c").unwrap());
}

#[test]
fn snap_redirect_out() {
    assert_debug_snapshot!(parse("echo hi > out.txt").unwrap());
}

#[test]
fn snap_redirect_append() {
    assert_debug_snapshot!(parse("echo hi >> log.txt").unwrap());
}

#[test]
fn snap_redirect_in() {
    assert_debug_snapshot!(parse("cat < in.txt").unwrap());
}

#[test]
fn snap_full_acceptance() {
    assert_debug_snapshot!(
        parse(r#"echo "hello world" | grep h > out.txt && cat out.txt"#).unwrap()
    );
}

#[test]
fn snap_multiple_redirects() {
    assert_debug_snapshot!(parse("cmd < in.txt > out.txt").unwrap());
}

#[test]
fn snap_empty() {
    assert_debug_snapshot!(parse("").unwrap());
}

#[test]
fn snap_parse_error_missing_command() {
    assert_debug_snapshot!(parse(">").unwrap_err());
}

#[test]
fn snap_parse_error_lex_propagated() {
    assert_debug_snapshot!(parse("'unterminated").unwrap_err());
}
