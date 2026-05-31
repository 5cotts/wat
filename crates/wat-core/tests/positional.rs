//! Tier 6 / Phase A: positional parameters, `$@`/`$*`/`$#`/`$0`, `shift`, `set --`.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn set_and_positional() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "set -- a b c; echo $1 $2 $3"), "a b c\n");
    assert_eq!(feed(&mut sh, "echo $#"), "3\n");
}

#[test]
fn set_without_dashes() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "set x y; echo $1-$2 $#"), "x-y 2\n");
}

#[test]
fn shift_drops_params() {
    let mut sh = shell();
    feed(&mut sh, "set -- a b c");
    assert_eq!(feed(&mut sh, "shift; echo $1 $#"), "b 2\n");
    assert_eq!(feed(&mut sh, "shift 2; echo [$1] $#"), "[] 0\n");
}

#[test]
fn shift_out_of_range_errors() {
    let mut sh = shell();
    feed(&mut sh, "set -- a");
    let out = feed(&mut sh, "shift 5");
    assert!(out.contains("out of range"), "got: {:?}", out);
    assert_eq!(feed(&mut sh, "echo $?").trim(), "1");
}

#[test]
fn at_expands_to_separate_words() {
    // Each param is its own word, so a param with spaces stays intact.
    let mut sh = shell();
    feed(&mut sh, "set -- \"a b\" c");
    assert_eq!(
        feed(&mut sh, "for x in \"$@\"; do echo [$x]; done"),
        "[a b]\n[c]\n"
    );
}

#[test]
fn star_joins_into_one_word() {
    let mut sh = shell();
    feed(&mut sh, "set -- a b c");
    assert_eq!(feed(&mut sh, "echo \"$*\""), "a b c\n");
}

#[test]
fn at_in_argument_position_joins_adjacent_literals() {
    let mut sh = shell();
    feed(&mut sh, "set -- a b");
    // pre + (a, b) + post -> "prea", "bpost"
    assert_eq!(feed(&mut sh, "echo pre$@post"), "prea bpost\n");
}

#[test]
fn empty_params() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo $#"), "0\n");
    assert_eq!(feed(&mut sh, "echo [$1]"), "[]\n");
    // A lone $@ with no params contributes no argument.
    assert_eq!(feed(&mut sh, "echo a $@ b"), "a b\n");
}

#[test]
fn braced_multidigit_positional() {
    let mut sh = shell();
    feed(&mut sh, "set -- 1 2 3 4 5 6 7 8 9 ten");
    assert_eq!(feed(&mut sh, "echo ${10}"), "ten\n");
}

#[test]
fn arg0_default() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo $0"), "wat\n");
}
