//! Tier 6 / Phase F: here-documents (`<<`, `<<-`, quoted) and here-strings.

use wat_core::{ParseStatus, Shell};

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn heredoc_feeds_body_to_stdin() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "cat <<EOF\nhello\nworld\nEOF"),
        "hello\nworld\n"
    );
}

#[test]
fn heredoc_expands_variables() {
    let mut sh = shell();
    feed(&mut sh, "X=there");
    assert_eq!(feed(&mut sh, "cat <<EOF\nhello $X\nEOF"), "hello there\n");
}

#[test]
fn quoted_delimiter_is_literal() {
    let mut sh = shell();
    feed(&mut sh, "X=there");
    assert_eq!(feed(&mut sh, "cat <<'EOF'\nhello $X\nEOF"), "hello $X\n");
}

#[test]
fn double_quoted_delimiter_is_literal() {
    let mut sh = shell();
    feed(&mut sh, "X=there");
    assert_eq!(feed(&mut sh, "cat <<\"EOF\"\nhi $X\nEOF"), "hi $X\n");
}

#[test]
fn dash_strips_leading_tabs() {
    let mut sh = shell();
    // Both the body lines and the closing delimiter may be tab-indented.
    let out = feed(&mut sh, "cat <<-EOF\n\t\tindented\n\tline two\n\tEOF");
    assert_eq!(out, "indented\nline two\n");
}

#[test]
fn plain_heredoc_keeps_leading_tabs() {
    let mut sh = shell();
    let out = feed(&mut sh, "cat <<EOF\n\tindented\nEOF");
    assert_eq!(out, "\tindented\n");
}

#[test]
fn empty_heredoc_body() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "cat <<EOF\nEOF"), "");
}

#[test]
fn heredoc_with_command_substitution() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "cat <<EOF\n$(echo nested)\nEOF"), "nested\n");
}

#[test]
fn here_string_feeds_word_plus_newline() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "cat <<<\"a b c\""), "a b c\n");
}

#[test]
fn here_string_expands_variable() {
    let mut sh = shell();
    feed(&mut sh, "name=wat");
    assert_eq!(feed(&mut sh, "cat <<<\"hi $name\""), "hi wat\n");
}

#[test]
fn here_string_into_wc() {
    let mut sh = shell();
    // One line in → wc -l reports 1.
    assert_eq!(feed(&mut sh, "wc -l <<<hello").trim(), "1");
}

#[test]
fn heredoc_into_pipeline() {
    let mut sh = shell();
    let out = feed(&mut sh, "cat <<EOF | grep two\none\ntwo\nthree\nEOF");
    assert_eq!(out, "two\n");
}

#[test]
fn unterminated_heredoc_is_incomplete() {
    let sh = shell();
    assert_eq!(sh.parse_status("cat <<EOF\nhello"), ParseStatus::Incomplete);
}

#[test]
fn heredoc_before_body_is_incomplete() {
    let sh = shell();
    // The introducing line is complete but the body has not begun.
    assert_eq!(sh.parse_status("cat <<EOF"), ParseStatus::Incomplete);
}

#[test]
fn complete_heredoc_parses() {
    let sh = shell();
    assert_eq!(sh.parse_status("cat <<EOF\nhi\nEOF"), ParseStatus::Complete);
}
