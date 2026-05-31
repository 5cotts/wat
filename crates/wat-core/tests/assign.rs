//! Variable assignment statements (`x=value`) and transient prefixes
//! (`x=value cmd`). Uses only builtins so it runs under the default feature set.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn assignment_persists_in_session() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "x=5"), ""); // no output
    assert_eq!(feed(&mut sh, "echo $x"), "5\n");
}

#[test]
fn assignment_value_is_expanded() {
    let mut sh = shell();
    feed(&mut sh, "x=hi");
    feed(&mut sh, "y=$x-there");
    assert_eq!(feed(&mut sh, "echo $y"), "hi-there\n");
}

#[test]
fn assignment_from_command_substitution() {
    let mut sh = shell();
    feed(&mut sh, "d=$(echo hello)");
    assert_eq!(feed(&mut sh, "echo $d"), "hello\n");
}

#[test]
fn assignment_value_is_not_field_split() {
    // RHS expansion does not word-split, so the double space is preserved.
    let mut sh = shell();
    feed(&mut sh, "x=$(echo 'a  b')");
    assert_eq!(feed(&mut sh, "echo \"$x\""), "a  b\n");
}

#[test]
fn assignment_from_arithmetic() {
    let mut sh = shell();
    feed(&mut sh, "n=$((6 * 7))");
    assert_eq!(feed(&mut sh, "echo $n"), "42\n");
}

#[test]
fn empty_assignment() {
    let mut sh = shell();
    feed(&mut sh, "x=");
    assert_eq!(feed(&mut sh, "echo [$x]"), "[]\n");
}

#[test]
fn arg_looking_like_assignment_after_name_is_arg() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo x=5"), "x=5\n");
    // ...and it must NOT have set x.
    assert_eq!(feed(&mut sh, "echo [$x]"), "[]\n");
}

#[test]
fn transient_assignment_visible_to_command_then_reverted() {
    // `env` is a builtin that prints the shell env. A transient FOO=bar is
    // visible to it, then gone afterward.
    let mut sh = shell();
    let during = feed(&mut sh, "FOO=bar env");
    assert!(
        during.lines().any(|l| l == "FOO=bar"),
        "expected FOO=bar during command, got: {:?}",
        during
    );
    let after = feed(&mut sh, "env");
    assert!(
        !after.lines().any(|l| l == "FOO=bar"),
        "FOO should not persist, got: {:?}",
        after
    );
}

#[test]
fn assignment_prefix_does_not_affect_line_expansion() {
    // POSIX: the prefix does not change expansion of the rest of the command
    // line, so `$x` here expands to the *old* value.
    let mut sh = shell();
    feed(&mut sh, "x=1");
    assert_eq!(feed(&mut sh, "x=5 echo $x"), "1\n");
    // And the transient assignment did not persist.
    assert_eq!(feed(&mut sh, "echo $x"), "1\n");
}

#[test]
fn pure_assignment_exit_code() {
    let mut sh = shell();
    feed(&mut sh, "y=5");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "0");
    // A failed command substitution in the value surfaces as the exit status.
    feed(&mut sh, "z=$(false)");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "1");
}
