//! Tier 6 / Phase G: quoting semantics. Single quotes are fully literal
//! (no `$`/`` ` ``/`~`/glob expansion); double quotes allow `$` but suppress
//! word splitting and globbing.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn single_quotes_suppress_variable_expansion() {
    let mut sh = shell();
    feed(&mut sh, "x=hi");
    assert_eq!(feed(&mut sh, "echo '$x'"), "$x\n");
}

#[test]
fn single_quotes_suppress_command_substitution() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo '$(echo nope)'"), "$(echo nope)\n");
}

#[test]
fn single_quotes_suppress_arithmetic() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo '$((1+1))'"), "$((1+1))\n");
}

#[test]
fn single_quotes_suppress_tilde() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo '~'"), "~\n");
}

#[test]
fn single_quotes_suppress_glob() {
    let mut sh = shell();
    // The seeded VFS has files in $HOME; '*' must stay literal.
    assert_eq!(feed(&mut sh, "echo '*'"), "*\n");
}

#[test]
fn double_quotes_allow_variable_expansion() {
    let mut sh = shell();
    feed(&mut sh, "x=hi");
    assert_eq!(feed(&mut sh, "echo \"$x\""), "hi\n");
}

#[test]
fn double_quotes_suppress_glob() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo \"*\""), "*\n");
}

#[test]
fn double_quotes_keep_value_as_single_field() {
    let mut sh = shell();
    feed(&mut sh, "x='a   b'");
    // The spaces inside the variable's value survive (no splitting) only when
    // the expansion is double-quoted.
    assert_eq!(feed(&mut sh, "echo \"$x\""), "a   b\n");
}

#[test]
fn double_quotes_with_command_substitution() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo \"got $(echo X)\""), "got X\n");
}

#[test]
fn mixed_quoting_concatenates() {
    let mut sh = shell();
    feed(&mut sh, "x=world");
    assert_eq!(feed(&mut sh, "echo 'hello '\"$x\"'!'"), "hello world!\n");
}

#[test]
fn quoted_keyword_is_a_command_name() {
    let mut sh = shell();
    // A quoted `if` is not the keyword; it's looked up as a command.
    let out = feed(&mut sh, "'if'");
    assert!(out.contains("command not found"), "got: {:?}", out);
}

#[test]
fn empty_single_quotes_are_an_empty_field() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo a''b"), "ab\n");
}

#[test]
fn literal_dollar_amid_text() {
    let mut sh = shell();
    feed(&mut sh, "p=path");
    assert_eq!(feed(&mut sh, "echo 'before $p after'"), "before $p after\n");
}

#[test]
fn double_quote_glob_in_pipeline_path_stays_literal() {
    let mut sh = shell();
    // Used as an argument that would otherwise glob-expand against the VFS.
    assert_eq!(feed(&mut sh, "echo \"a*b\""), "a*b\n");
}
