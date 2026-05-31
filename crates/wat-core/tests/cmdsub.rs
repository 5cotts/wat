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
fn cmdsub_quoted_preserves_interior_newline() {
    // Quoted: only *trailing* newlines are stripped; an interior newline
    // survives (no field splitting inside double quotes).
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo \"[$(echo a; echo b)]\""), "[a\nb]\n");
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

// ── Tier 4 / Phase C: field splitting of unquoted substitutions ──────────

#[test]
fn split_unquoted_collapses_whitespace() {
    // Unquoted output "a  b" (two spaces) splits into [a, b]; echo rejoins with
    // a single space — proving the double space was a field separator, not
    // preserved literal text.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo $(echo 'a  b')"), "a b\n");
}

#[test]
fn quoted_preserves_internal_whitespace() {
    // Quoted: the same output stays one field, double space intact.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo \"$(echo 'a  b')\""), "a  b\n");
}

#[test]
fn split_into_multiple_fields() {
    let mut sh = shell();
    // Three fields; echo joins them with single spaces.
    assert_eq!(feed(&mut sh, "echo [$(echo a b c)]"), "[a b c]\n");
}

#[test]
fn split_unquoted_on_interior_newline() {
    // Unquoted substitution splits on newlines too (IFS), unlike the quoted
    // form which preserves them.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo [$(echo a; echo b)]"), "[a b]\n");
}

#[test]
fn split_with_adjacent_literals() {
    // Literals join the first/last field of the split: x|a b|y -> "xa","by".
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo x$(echo 'a b')y"), "xa by\n");
}

#[test]
fn empty_unquoted_sub_contributes_no_field() {
    // The middle word `$(true)` expands to zero fields, so echo sees two args.
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo a $(true) b"), "a b\n");
}

#[test]
fn split_result_is_globbed() {
    // A split field that is a glob pattern is path-expanded by the caller.
    // Build the pattern via a file's *contents* (cat) so it isn't globbed by
    // the inner command — isolating the outer, post-split glob step.
    let mut sh = shell();
    feed(&mut sh, "mkdir gd");
    feed(&mut sh, "cd gd");
    feed(&mut sh, "echo '*.md' > pat"); // no .md files yet -> literal "*.md"
    feed(&mut sh, "touch x.md");
    feed(&mut sh, "touch y.md");
    let out = feed(&mut sh, "echo $(cat pat)");
    assert!(
        out.contains("x.md"),
        "glob not applied to sub result: {:?}",
        out
    );
    assert!(
        out.contains("y.md"),
        "glob not applied to sub result: {:?}",
        out
    );
    // And the raw pattern should be gone (it matched).
    assert!(!out.contains('*'), "pattern left unexpanded: {:?}", out);
}
