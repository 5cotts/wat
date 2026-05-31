//! Tier 5 / Phase C: `while` / `until` / `for` loops + `break` / `continue`.
//! Uses exit-code conditions and `break` (the `test` builtin arrives in Phase
//! E, which enables counter-driven loop tests).

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn for_iterates_a_list() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "for x in a b c; do echo $x; done"),
        "a\nb\nc\n"
    );
}

#[test]
fn for_with_command_substitution() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "for f in $(echo 1 2 3); do echo $f; done"),
        "1\n2\n3\n"
    );
}

#[test]
fn for_with_glob() {
    let mut sh = shell();
    feed(&mut sh, "mkdir d");
    feed(&mut sh, "cd d");
    feed(&mut sh, "touch a.txt");
    feed(&mut sh, "touch b.txt");
    // glob expands to one iteration per match (paths as the VFS reports them).
    let out = feed(&mut sh, "for f in *.txt; do echo $f; done");
    assert_eq!(
        out.lines().count(),
        2,
        "expected two matches, got: {:?}",
        out
    );
    assert!(
        out.contains("a.txt") && out.contains("b.txt"),
        "got: {:?}",
        out
    );
}

#[test]
fn for_empty_list_runs_zero_times() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "for x in; do echo $x; done"), "");
}

#[test]
fn while_false_runs_zero_times() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "while false; do echo x; done"), "");
}

#[test]
fn until_true_runs_zero_times() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "until true; do echo x; done"), "");
}

#[test]
fn while_true_with_break() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "while true; do echo hi; break; done"), "hi\n");
}

#[test]
fn until_false_with_break() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "until false; do echo hi; break; done"),
        "hi\n"
    );
}

#[test]
fn break_stops_the_loop() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "for x in a b c; do echo $x; break; done"),
        "a\n"
    );
}

#[test]
fn continue_skips_rest_of_body() {
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "for x in a b c; do echo pre$x; continue; echo post$x; done"
        ),
        "prea\npreb\nprec\n"
    );
}

#[test]
fn break_only_innermost_loop() {
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "for x in a b; do for y in 1 2; do echo $x$y; break; done; done"
        ),
        "a1\nb1\n"
    );
}

#[test]
fn break_outside_loop_is_diagnostic_not_crash() {
    let mut sh = shell();
    let out = feed(&mut sh, "break");
    assert!(out.contains("only meaningful"), "got: {:?}", out);
    assert_eq!(feed(&mut sh, "echo $?").trim(), "0");
}

#[test]
fn loop_body_assignment_persists() {
    let mut sh = shell();
    feed(&mut sh, "for x in a b c; do last=$x; done");
    assert_eq!(feed(&mut sh, "echo $last"), "c\n");
}

#[test]
fn nested_loops_full_product() {
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "for x in 1 2; do for y in a b; do echo $x$y; done; done"
        ),
        "1a\n1b\n2a\n2b\n"
    );
}
