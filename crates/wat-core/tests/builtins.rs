use wat_core::Shell;

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn echo_hello_world() {
    let mut sh = Shell::new();
    assert_eq!(feed(&mut sh, "echo hello world"), "hello world\n");
}

#[test]
fn echo_expands_home() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "echo $HOME");
    assert_eq!(out.trim(), "/home/5cotts");
}

#[test]
fn echo_expands_tilde() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "echo ~");
    assert_eq!(out.trim(), "/home/5cotts");
}

#[test]
fn pwd_initial() {
    let mut sh = Shell::new();
    assert_eq!(feed(&mut sh, "pwd"), "/home/5cotts\n");
}

#[test]
fn cd_and_pwd() {
    let mut sh = Shell::new();
    feed(&mut sh, "cd /");
    assert_eq!(feed(&mut sh, "pwd"), "/\n");
}

#[test]
fn cd_updates_prompt() {
    let mut sh = Shell::new();
    feed(&mut sh, "cd /tmp");
    assert_eq!(sh.prompt(), "5cotts@zo /tmp % ");
}

#[test]
fn cd_oldpwd() {
    let mut sh = Shell::new();
    feed(&mut sh, "cd /tmp");
    feed(&mut sh, "cd -");
    assert_eq!(feed(&mut sh, "pwd"), "/home/5cotts\n");
}

#[test]
fn export_and_echo() {
    let mut sh = Shell::new();
    feed(&mut sh, "export MY_VAR=hello");
    assert_eq!(feed(&mut sh, "echo $MY_VAR"), "hello\n");
}

#[test]
fn unset_removes_var() {
    let mut sh = Shell::new();
    feed(&mut sh, "export FOO=bar");
    feed(&mut sh, "unset FOO");
    assert_eq!(feed(&mut sh, "echo $FOO"), "\n");
}

#[test]
fn exit_sets_flag() {
    let mut sh = Shell::new();
    sh.feed("exit 0");
    assert!(sh.exit_requested);
    assert_eq!(sh.last_exit_code(), 0);
}

#[test]
fn exit_42() {
    let mut sh = Shell::new();
    sh.feed("exit 42");
    assert_eq!(sh.last_exit_code(), 42);
}

#[test]
fn dollar_question_after_true() {
    let mut sh = Shell::new();
    feed(&mut sh, "true");
    assert_eq!(feed(&mut sh, "echo $?"), "0\n");
}

#[test]
fn dollar_question_after_false() {
    let mut sh = Shell::new();
    feed(&mut sh, "false");
    assert_eq!(feed(&mut sh, "echo $?"), "1\n");
}

#[test]
fn and_operator_short_circuits() {
    let mut sh = Shell::new();
    // false && echo yes — "yes" should NOT appear
    let out = feed(&mut sh, "false && echo yes");
    assert!(!out.contains("yes"));
}

#[test]
fn and_operator_runs_on_success() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "true && echo yes");
    assert!(out.contains("yes"));
}

#[test]
fn or_operator_short_circuits() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "true || echo fallback");
    assert!(!out.contains("fallback"));
}

#[test]
fn or_operator_runs_on_failure() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "false || echo fallback");
    assert!(out.contains("fallback"));
}

#[test]
fn semicolon_runs_both() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "echo a ; echo b");
    assert!(out.contains("a\n"));
    assert!(out.contains("b\n"));
}

#[test]
fn unknown_command_returns_127() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "nonexistent_command");
    assert!(out.contains("command not found"));
    assert_eq!(sh.last_exit_code(), 127);
}

#[test]
fn parse_error_does_not_crash() {
    let mut sh = Shell::new();
    let out = feed(&mut sh, "'unterminated");
    assert!(!out.is_empty()); // error message
}

#[test]
fn braced_var_expansion() {
    let mut sh = Shell::new();
    feed(&mut sh, "export GREET=hi");
    assert_eq!(feed(&mut sh, "echo ${GREET}there"), "hithere\n");
}

#[test]
fn cd_acceptance() {
    // `cd /tmp && pwd` should print /tmp
    let mut sh = Shell::new();
    let out = feed(&mut sh, "cd /tmp && pwd");
    assert_eq!(out.trim(), "/tmp");
}

#[test]
fn false_then_dollar_question() {
    // `false; echo $?` should print 1
    let mut sh = Shell::new();
    let out = feed(&mut sh, "false; echo $?");
    assert!(out.contains('1'));
}
