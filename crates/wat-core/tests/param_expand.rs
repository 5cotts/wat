//! Tier 6 / Phase D: `${...}` parameter-expansion operators.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn use_default_when_unset() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo ${X:-fallback}"), "fallback\n");
}

#[test]
fn use_default_when_set() {
    let mut sh = shell();
    feed(&mut sh, "X=value");
    assert_eq!(feed(&mut sh, "echo ${X:-fallback}"), "value\n");
}

#[test]
fn colon_dash_treats_empty_as_unset() {
    let mut sh = shell();
    feed(&mut sh, "X=");
    assert_eq!(feed(&mut sh, "echo ${X:-fallback}"), "fallback\n");
}

#[test]
fn plain_dash_keeps_empty_value() {
    let mut sh = shell();
    feed(&mut sh, "X=");
    // No colon: empty counts as "set", so the default is not used.
    assert_eq!(feed(&mut sh, "echo [${X-fallback}]"), "[]\n");
}

#[test]
fn assign_default_sets_variable() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo ${X:=assigned}"), "assigned\n");
    // The assignment persisted into the environment.
    assert_eq!(feed(&mut sh, "echo $X"), "assigned\n");
}

#[test]
fn assign_default_noop_when_set() {
    let mut sh = shell();
    feed(&mut sh, "X=orig");
    assert_eq!(feed(&mut sh, "echo ${X:=new}"), "orig\n");
    assert_eq!(feed(&mut sh, "echo $X"), "orig\n");
}

#[test]
fn error_if_unset_writes_message() {
    let mut sh = shell();
    let out = feed(&mut sh, "echo ${MISSING:?required}");
    assert!(out.contains("MISSING: required"), "got: {:?}", out);
}

#[test]
fn error_if_unset_uses_value_when_set() {
    let mut sh = shell();
    feed(&mut sh, "MISSING=here");
    assert_eq!(feed(&mut sh, "echo ${MISSING:?required}"), "here\n");
}

#[test]
fn use_alternate_when_set() {
    let mut sh = shell();
    feed(&mut sh, "X=value");
    assert_eq!(feed(&mut sh, "echo ${X:+alt}"), "alt\n");
}

#[test]
fn use_alternate_empty_when_unset() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo [${X:+alt}]"), "[]\n");
}

#[test]
fn length_of_value() {
    let mut sh = shell();
    feed(&mut sh, "X=hello");
    assert_eq!(feed(&mut sh, "echo ${#X}"), "5\n");
}

#[test]
fn length_of_unset_is_zero() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo ${#X}"), "0\n");
}

#[test]
fn remove_shortest_prefix() {
    let mut sh = shell();
    feed(&mut sh, "F=a.b.c");
    assert_eq!(feed(&mut sh, "echo ${F#*.}"), "b.c\n");
}

#[test]
fn remove_longest_prefix() {
    let mut sh = shell();
    feed(&mut sh, "F=a.b.c");
    assert_eq!(feed(&mut sh, "echo ${F##*.}"), "c\n");
}

#[test]
fn remove_shortest_suffix() {
    let mut sh = shell();
    feed(&mut sh, "F=a.b.c");
    assert_eq!(feed(&mut sh, "echo ${F%.*}"), "a.b\n");
}

#[test]
fn remove_longest_suffix() {
    let mut sh = shell();
    feed(&mut sh, "F=a.b.c");
    assert_eq!(feed(&mut sh, "echo ${F%%.*}"), "a\n");
}

#[test]
fn trim_literal_extension() {
    let mut sh = shell();
    feed(&mut sh, "F=archive.tar.gz");
    assert_eq!(feed(&mut sh, "echo ${F%.gz}"), "archive.tar\n");
}

#[test]
fn default_can_reference_another_variable() {
    let mut sh = shell();
    feed(&mut sh, "Y=fromY");
    assert_eq!(feed(&mut sh, "echo ${X:-$Y}"), "fromY\n");
}

#[test]
fn length_of_positional() {
    let mut sh = shell();
    feed(&mut sh, "set -- abcd");
    assert_eq!(feed(&mut sh, "echo ${#1}"), "4\n");
}

#[test]
fn no_match_leaves_value_unchanged() {
    let mut sh = shell();
    feed(&mut sh, "F=plain");
    assert_eq!(feed(&mut sh, "echo ${F#x}"), "plain\n");
    assert_eq!(feed(&mut sh, "echo ${F%x}"), "plain\n");
}
