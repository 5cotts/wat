//! Tier 5 / Phase E: the `test` / `[` builtin.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

/// Evaluate a condition and map it to "T"/"F" via && / ||.
fn cond(sh: &mut Shell, expr: &str) -> String {
    feed(sh, &format!("{} && echo T || echo F", expr))
}

#[test]
fn integer_comparisons() {
    let mut sh = shell();
    assert_eq!(cond(&mut sh, "test 1 -lt 2"), "T\n");
    assert_eq!(cond(&mut sh, "test 2 -lt 1"), "F\n");
    assert_eq!(cond(&mut sh, "test 3 -ge 3"), "T\n");
    assert_eq!(cond(&mut sh, "test 3 -eq 4"), "F\n");
    assert_eq!(cond(&mut sh, "test 5 -ne 4"), "T\n");
}

#[test]
fn string_tests() {
    let mut sh = shell();
    assert_eq!(cond(&mut sh, "test -z \"\""), "T\n");
    assert_eq!(cond(&mut sh, "test -n abc"), "T\n");
    assert_eq!(cond(&mut sh, "test abc = abc"), "T\n");
    assert_eq!(cond(&mut sh, "test abc = xyz"), "F\n");
    assert_eq!(cond(&mut sh, "test abc != xyz"), "T\n");
    // A bare non-empty string is true; empty is false.
    assert_eq!(cond(&mut sh, "test hello"), "T\n");
    assert_eq!(cond(&mut sh, "test \"\""), "F\n");
}

#[test]
fn file_tests_via_vfs() {
    let mut sh = shell();
    feed(&mut sh, "touch f");
    feed(&mut sh, "mkdir d");
    assert_eq!(cond(&mut sh, "test -e f"), "T\n");
    assert_eq!(cond(&mut sh, "test -f f"), "T\n");
    assert_eq!(cond(&mut sh, "test -d f"), "F\n"); // a file is not a dir
    assert_eq!(cond(&mut sh, "test -d d"), "T\n");
    assert_eq!(cond(&mut sh, "test -f d"), "F\n"); // a dir is not a regular file
    assert_eq!(cond(&mut sh, "test -e nope"), "F\n");
}

#[test]
fn negation() {
    let mut sh = shell();
    assert_eq!(cond(&mut sh, "test ! -f /nope"), "T\n");
    assert_eq!(cond(&mut sh, "test ! 1 -lt 2"), "F\n");
}

#[test]
fn bracket_form() {
    let mut sh = shell();
    assert_eq!(cond(&mut sh, "[ 1 -lt 2 ]"), "T\n");
    assert_eq!(cond(&mut sh, "[ abc = abc ]"), "T\n");
}

#[test]
fn bracket_missing_close_is_usage_error() {
    let mut sh = shell();
    let out = feed(&mut sh, "[ 1 -lt 2");
    assert!(out.contains("missing ']'"), "got: {:?}", out);
    assert_eq!(feed(&mut sh, "echo $?").trim(), "2");
}

#[test]
fn non_integer_operand_is_usage_error() {
    let mut sh = shell();
    let out = feed(&mut sh, "test abc -lt 2");
    assert!(
        out.contains("integer expression expected"),
        "got: {:?}",
        out
    );
    assert_eq!(feed(&mut sh, "echo $?").trim(), "2");
}

#[test]
fn drives_an_if() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "if test 3 -gt 2; then echo y; else echo n; fi"),
        "y\n"
    );
}

#[test]
fn drives_a_counter_while_loop() {
    // The capstone: test + arithmetic + assignment in a while loop.
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "i=0; while test $i -lt 3; do echo $i; i=$((i + 1)); done"
        ),
        "0\n1\n2\n"
    );
}
