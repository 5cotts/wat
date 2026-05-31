//! Tier 5 / Phase B: `if` / `elif` / `else` / `fi`. Single-line forms run
//! under the default feature set via `feed` (builtins only).

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn if_true_runs_then() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "if true; then echo yes; fi"), "yes\n");
}

#[test]
fn if_false_skips_then() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "if false; then echo yes; fi"), "");
}

#[test]
fn if_else() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "if false; then echo a; else echo b; fi"),
        "b\n"
    );
}

#[test]
fn if_elif_else() {
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "if false; then echo a; elif true; then echo b; else echo c; fi"
        ),
        "b\n"
    );
    // Falls through to else when no branch matches.
    assert_eq!(
        feed(
            &mut sh,
            "if false; then echo a; elif false; then echo b; else echo c; fi"
        ),
        "c\n"
    );
}

#[test]
fn if_condition_is_a_pipeline() {
    // The condition is a real pipeline; its exit code gates the branch. `grep`
    // here also echoes the matched line, so it appears before `matched`.
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "if echo hi | grep hi; then echo matched; fi"),
        "hi\nmatched\n"
    );
    // A failing pipeline condition skips the branch (grep finds nothing → 1).
    assert_eq!(
        feed(&mut sh, "if echo hi | grep nope; then echo matched; fi"),
        ""
    );
}

#[test]
fn if_exit_code_reflects_body() {
    let mut sh = shell();
    feed(&mut sh, "if true; then false; fi");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "1");
    // A not-taken if with no else is exit 0.
    feed(&mut sh, "if false; then echo x; fi");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "0");
}

#[test]
fn if_is_a_keyword_only_in_command_position() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo if"), "if\n");
    assert_eq!(feed(&mut sh, "echo then fi"), "then fi\n");
}

#[test]
fn nested_if() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "if true; then if true; then echo deep; fi; fi"),
        "deep\n"
    );
}

#[test]
fn assignment_inside_if_persists() {
    // Compounds run in the current shell (no subshell).
    let mut sh = shell();
    feed(&mut sh, "if true; then x=42; fi");
    assert_eq!(feed(&mut sh, "echo $x"), "42\n");
}

#[test]
fn multiline_via_newline_separators() {
    // The lexer treats newlines as separators, so a multi-line `if` provided
    // as one string already parses (Phase F adds REPL continuation).
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "if true\nthen\necho hi\nfi"), "hi\n");
}
