//! Tier 6 / Phase C: functions, `return`, and brace groups.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn define_and_call() {
    let mut sh = shell();
    feed(&mut sh, "greet() { echo \"hi $1\"; }");
    assert_eq!(feed(&mut sh, "greet bob"), "hi bob\n");
}

#[test]
fn function_keyword_form() {
    let mut sh = shell();
    feed(&mut sh, "function f { echo alt; }");
    assert_eq!(feed(&mut sh, "f"), "alt\n");
}

#[test]
fn space_before_parens_form() {
    let mut sh = shell();
    feed(&mut sh, "f () { echo spaced; }");
    assert_eq!(feed(&mut sh, "f"), "spaced\n");
}

#[test]
fn return_sets_status() {
    let mut sh = shell();
    feed(&mut sh, "f() { return 3; }");
    feed(&mut sh, "f");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "3");
}

#[test]
fn return_stops_body_early() {
    let mut sh = shell();
    feed(&mut sh, "f() { echo a; return; echo b; }");
    assert_eq!(feed(&mut sh, "f"), "a\n");
}

#[test]
fn positional_params_restored_after_call() {
    let mut sh = shell();
    feed(&mut sh, "set -- outer");
    feed(&mut sh, "f() { echo inner=$1; }");
    assert_eq!(feed(&mut sh, "f arg"), "inner=arg\n");
    // The caller's $1 is unchanged.
    assert_eq!(feed(&mut sh, "echo $1"), "outer\n");
}

#[test]
fn function_sees_all_args_via_at() {
    let mut sh = shell();
    feed(&mut sh, "f() { for x in \"$@\"; do echo [$x]; done; }");
    assert_eq!(feed(&mut sh, "f a b c"), "[a]\n[b]\n[c]\n");
}

#[test]
fn recursion_with_base_case() {
    // countdown N: prints N..1 using arithmetic + test (recursion).
    let mut sh = shell();
    feed(
        &mut sh,
        "down() { if test $1 -gt 0; then echo $1; down $(($1 - 1)); fi; }",
    );
    assert_eq!(feed(&mut sh, "down 3"), "3\n2\n1\n");
}

#[test]
fn brace_group_runs_in_current_shell() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "{ echo a; echo b; }"), "a\nb\n");
    feed(&mut sh, "{ x=99; }");
    assert_eq!(feed(&mut sh, "echo $x"), "99\n");
}

#[test]
fn return_outside_function_errors() {
    let mut sh = shell();
    let out = feed(&mut sh, "return 2");
    assert!(out.contains("can only"), "got: {:?}", out);
}

#[test]
fn function_can_use_control_flow_and_pipes() {
    let mut sh = shell();
    feed(
        &mut sh,
        "classify() { case $1 in *.txt) echo text;; *) echo other;; esac; }",
    );
    assert_eq!(feed(&mut sh, "classify a.txt"), "text\n");
    assert_eq!(feed(&mut sh, "classify a.md"), "other\n");
}

#[test]
fn redefining_a_function() {
    let mut sh = shell();
    feed(&mut sh, "f() { echo one; }");
    feed(&mut sh, "f() { echo two; }");
    assert_eq!(feed(&mut sh, "f"), "two\n");
}
