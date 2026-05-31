//! Tier 4 / Phase B: command substitution evaluation (single-word; no field
//! splitting yet). Uses only builtins inside `$(...)` so the tests run under
//! the default feature set (no `native-proc` needed) and on every target.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn cmdsub_basic() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo $(echo hi)"), "hi\n");
}

#[test]
fn cmdsub_strips_trailing_newlines() {
    // Inner stdout is "hi\n\n" (echo hi, then a blank echo); both trailing
    // newlines are stripped, leaving a single `hi` arg.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo [$(echo hi; echo)]"), "[hi]\n");
}

#[test]
fn cmdsub_preserves_interior_newline() {
    // Only *trailing* newlines are stripped; an interior newline survives.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo [$(echo a; echo b)]"), "[a\nb]\n");
}

#[test]
fn cmdsub_in_argument_position() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo pre$(echo X)post"), "preXpost\n");
}

#[test]
fn cmdsub_quoted_is_single_word() {
    // Phase B does not split anyway, but a quoted substitution must stay one
    // word with interior spaces preserved.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo \"$(echo a b)\""), "a b\n");
}

#[test]
fn cmdsub_nested() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo $(echo $(echo deep))"), "deep\n");
}

#[test]
fn cmdsub_backticks() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo `echo hi`"), "hi\n");
}

#[test]
fn cmdsub_runs_a_builtin_pipeline() {
    // The inner source is a full pipeline, not just a single command.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo $(echo hello | grep hello)"), "hello\n");
}

#[test]
fn cmdsub_exit_code_is_outer_not_inner() {
    // `echo` succeeds even though the substitution ran `false`; $? is the
    // echo's status (0), not the substitution's.
    let mut sh = shell();
    feed(&mut sh, "echo $(false)");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "0");
    // A bare false still sets 1.
    feed(&mut sh, "false");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "1");
}

#[test]
fn cmdsub_stderr_passes_through() {
    // Inner command writes to stderr; that diagnostic must surface (feed
    // combines stdout then stderr), while its empty stdout is what substitutes.
    let mut sh = shell();
    let out = feed(&mut sh, "echo [$(cat /no/such/file)]");
    assert!(
        out.contains("[]"),
        "expected empty substitution, got: {:?}",
        out
    );
    assert!(
        out.contains("cat:"),
        "expected cat stderr to surface, got: {:?}",
        out
    );
}

#[test]
fn cmdsub_empty_output() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo [$(true)]"), "[]\n");
}

#[test]
fn cmdsub_depth_limit_does_not_crash() {
    // Deeply nested substitution must error gracefully, not blow the stack.
    let mut sh = shell();
    let mut s = String::from("echo ");
    for _ in 0..64 {
        s.push_str("$(echo ");
    }
    s.push('x');
    for _ in 0..64 {
        s.push(')');
    }
    // Just assert it returns without panicking.
    let _ = feed(&mut sh, &s);
}

#[test]
fn plain_command_unaffected_by_ctx_expansion() {
    // Regression: words without substitutions expand exactly as before.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo hello world"), "hello world\n");
    assert_eq!(feed(&mut sh, "echo $HOME").trim(), "/home/5cotts");
    assert_eq!(feed(&mut sh, "echo ~").trim(), "/home/5cotts");
}
