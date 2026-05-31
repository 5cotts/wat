//! Tier 6 / Phase E: scripting builtins (`:`, `printf`, `read`, `eval`,
//! `.`/`source`) and `set -e`/`-u`/`-x`.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn colon_is_noop_success() {
    let mut sh = shell();
    feed(&mut sh, ":");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "0");
}

#[test]
fn printf_basic_substitution() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "printf '%s-%s\\n' a b"), "a-b\n");
}

#[test]
fn printf_cycles_format_over_extra_args() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "printf '%s\\n' a b c"), "a\nb\nc\n");
}

#[test]
fn printf_integer_and_hex() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "printf '%d %x\\n' 255 255"), "255 ff\n");
}

#[test]
fn printf_literal_percent() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "printf '100%%\\n'"), "100%\n");
}

#[test]
fn printf_no_args_prints_once() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "printf 'hello\\n'"), "hello\n");
}

#[test]
fn read_splits_into_vars() {
    let mut sh = shell();
    feed(&mut sh, "printf 'one two three\\n' | read a b c");
    assert_eq!(feed(&mut sh, "echo $a $c").trim(), "one three");
}

#[test]
fn read_last_var_gets_remainder() {
    let mut sh = shell();
    feed(&mut sh, "printf 'one two three four\\n' | read a b");
    assert_eq!(feed(&mut sh, "echo $b").trim(), "two three four");
}

#[test]
fn read_default_reply() {
    let mut sh = shell();
    feed(&mut sh, "printf 'a line\\n' | read");
    assert_eq!(feed(&mut sh, "echo $REPLY").trim(), "a line");
}

#[test]
fn eval_runs_constructed_command() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "eval 'echo hi'"), "hi\n");
}

#[test]
fn eval_expands_then_runs() {
    let mut sh = shell();
    feed(&mut sh, "x=echo");
    assert_eq!(feed(&mut sh, "eval \"$x ok\""), "ok\n");
}

#[test]
fn eval_assignment_persists() {
    let mut sh = shell();
    feed(&mut sh, "eval 'y=fromeval'");
    assert_eq!(feed(&mut sh, "echo $y").trim(), "fromeval");
}

#[test]
fn source_runs_file_in_current_shell() {
    let mut sh = shell();
    feed(
        &mut sh,
        "printf 'greet() { echo hello; }\\nVAR=42\\n' > lib.sh",
    );
    feed(&mut sh, ". lib.sh");
    assert_eq!(feed(&mut sh, "greet"), "hello\n");
    assert_eq!(feed(&mut sh, "echo $VAR").trim(), "42");
}

#[test]
fn source_missing_file_errors() {
    let mut sh = shell();
    let out = feed(&mut sh, "source nope.sh");
    assert!(out.contains("source"), "got: {:?}", out);
    assert_eq!(feed(&mut sh, "echo $?").trim(), "1");
}

#[test]
fn errexit_aborts_list_on_failure() {
    let mut sh = shell();
    let out = feed(&mut sh, "set -e; false; echo after");
    assert!(!out.contains("after"), "got: {:?}", out);
}

#[test]
fn errexit_does_not_abort_in_if_condition() {
    let mut sh = shell();
    let out = feed(&mut sh, "set -e; if false; then echo yes; else echo no; fi");
    assert_eq!(out, "no\n");
}

#[test]
fn errexit_does_not_abort_on_or_operand() {
    let mut sh = shell();
    // `false` is an operand of `||`, so its failure is tolerated.
    let out = feed(&mut sh, "set -e; false || echo recovered");
    assert_eq!(out, "recovered\n");
}

#[test]
fn nounset_unset_variable_errors() {
    let mut sh = shell();
    let out = feed(&mut sh, "set -u; echo $MISSING; echo after");
    assert!(out.contains("unbound variable"), "got: {:?}", out);
    assert!(!out.contains("after"), "got: {:?}", out);
}

#[test]
fn nounset_allows_set_variable() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "set -u; X=ok; echo $X"), "ok\n");
}

#[test]
fn nounset_braced_form_errors() {
    let mut sh = shell();
    let out = feed(&mut sh, "set -u; echo ${MISSING}");
    assert!(out.contains("unbound variable"), "got: {:?}", out);
}

#[test]
fn xtrace_prints_commands_to_stderr() {
    let mut sh = shell();
    // The traced line is interleaved into feed()'s combined output.
    let out = feed(&mut sh, "set -x; echo hi");
    assert!(out.contains("+ echo hi"), "got: {:?}", out);
    assert!(out.contains("hi"), "got: {:?}", out);
}

#[test]
fn set_flags_do_not_clear_positionals() {
    let mut sh = shell();
    feed(&mut sh, "set -- a b c");
    feed(&mut sh, "set -x");
    feed(&mut sh, "set +x");
    assert_eq!(feed(&mut sh, "echo $1 $3").trim(), "a c");
}
