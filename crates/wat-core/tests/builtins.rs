use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn echo_hello_world() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "echo hello world"), "hello world\n");
}

#[test]
fn echo_expands_home() {
    let mut sh = shell();
    let out = feed(&mut sh, "echo $HOME");
    assert_eq!(out.trim(), "/home/5cotts");
}

#[test]
fn echo_expands_tilde() {
    let mut sh = shell();
    let out = feed(&mut sh, "echo ~");
    assert_eq!(out.trim(), "/home/5cotts");
}

#[test]
fn pwd_initial() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "pwd"), "/home/5cotts\n");
}

#[test]
fn cd_and_pwd() {
    let mut sh = shell();
    feed(&mut sh, "cd /");
    assert_eq!(feed(&mut sh, "pwd"), "/\n");
}

#[test]
fn cd_updates_prompt() {
    let mut sh = shell();
    // /tmp doesn't exist in MemoryVfs; cd to a seeded dir instead
    feed(&mut sh, "cd /etc");
    assert_eq!(sh.prompt(), "5cotts@zo /etc % ");
}

#[test]
fn cd_oldpwd() {
    let mut sh = shell();
    feed(&mut sh, "cd /etc");
    feed(&mut sh, "cd -");
    assert_eq!(feed(&mut sh, "pwd"), "/home/5cotts\n");
}

#[test]
fn cd_nonexistent_fails() {
    let mut sh = shell();
    let out = feed(&mut sh, "cd /does/not/exist");
    assert!(out.contains("No such file or directory"));
}

#[test]
fn export_and_echo() {
    let mut sh = shell();
    feed(&mut sh, "export MY_VAR=hello");
    assert_eq!(feed(&mut sh, "echo $MY_VAR"), "hello\n");
}

#[test]
fn unset_removes_var() {
    let mut sh = shell();
    feed(&mut sh, "export FOO=bar");
    feed(&mut sh, "unset FOO");
    assert_eq!(feed(&mut sh, "echo $FOO"), "\n");
}

#[test]
fn exit_sets_flag() {
    let mut sh = shell();
    sh.feed("exit 0");
    assert!(sh.exit_requested);
    assert_eq!(sh.last_exit_code(), 0);
}

#[test]
fn exit_42() {
    let mut sh = shell();
    sh.feed("exit 42");
    assert_eq!(sh.last_exit_code(), 42);
}

#[test]
fn dollar_question_after_true() {
    let mut sh = shell();
    feed(&mut sh, "true");
    assert_eq!(feed(&mut sh, "echo $?"), "0\n");
}

#[test]
fn dollar_question_after_false() {
    let mut sh = shell();
    feed(&mut sh, "false");
    assert_eq!(feed(&mut sh, "echo $?"), "1\n");
}

#[test]
fn and_operator_short_circuits() {
    let mut sh = shell();
    let out = feed(&mut sh, "false && echo yes");
    assert!(!out.contains("yes"));
}

#[test]
fn and_operator_runs_on_success() {
    let mut sh = shell();
    let out = feed(&mut sh, "true && echo yes");
    assert!(out.contains("yes"));
}

#[test]
fn or_operator_short_circuits() {
    let mut sh = shell();
    let out = feed(&mut sh, "true || echo fallback");
    assert!(!out.contains("fallback"));
}

#[test]
fn or_operator_runs_on_failure() {
    let mut sh = shell();
    let out = feed(&mut sh, "false || echo fallback");
    assert!(out.contains("fallback"));
}

#[test]
fn semicolon_runs_both() {
    let mut sh = shell();
    let out = feed(&mut sh, "echo a ; echo b");
    assert!(out.contains("a\n"));
    assert!(out.contains("b\n"));
}

#[test]
fn unknown_command_returns_127() {
    let mut sh = shell();
    let out = feed(&mut sh, "nonexistent_command");
    assert!(out.contains("command not found"));
    assert_eq!(sh.last_exit_code(), 127);
}

#[test]
fn parse_error_does_not_crash() {
    let mut sh = shell();
    let out = feed(&mut sh, "'unterminated");
    assert!(!out.is_empty());
}

#[test]
fn braced_var_expansion() {
    let mut sh = shell();
    feed(&mut sh, "export GREET=hi");
    assert_eq!(feed(&mut sh, "echo ${GREET}there"), "hithere\n");
}

#[test]
fn cd_acceptance() {
    let mut sh = shell();
    let out = feed(&mut sh, "cd /etc && pwd");
    assert_eq!(out.trim(), "/etc");
}

#[test]
fn false_then_dollar_question() {
    let mut sh = shell();
    let out = feed(&mut sh, "false; echo $?");
    assert!(out.contains('1'));
}

// ── Phase 3: VFS / file builtins ──────────────────────────────────────────

#[test]
fn mkdir_cd_touch_ls() {
    let mut sh = shell();
    feed(&mut sh, "mkdir /tmp");
    feed(&mut sh, "cd /tmp");
    feed(&mut sh, "touch bar");
    let out = feed(&mut sh, "ls");
    assert!(out.contains("bar"));
}

#[test]
fn cat_motd() {
    let mut sh = shell();
    let out = feed(&mut sh, "cat /etc/motd");
    assert!(out.contains("wat"));
}

#[test]
fn rm_rf_root_rejected() {
    let mut sh = shell();
    let out = feed(&mut sh, "rm -rf /");
    // Custom snark message
    assert!(!out.is_empty());
    // /etc/motd should still be readable (root wasn't deleted)
    let motd_out = feed(&mut sh, "cat /etc/motd");
    assert!(!motd_out.is_empty());
}

#[test]
fn ls_shows_files() {
    let mut sh = shell();
    feed(&mut sh, "mkdir /testdir");
    feed(&mut sh, "touch /testdir/file1");
    feed(&mut sh, "touch /testdir/file2");
    let out = feed(&mut sh, "ls /testdir");
    assert!(out.contains("file1"));
    assert!(out.contains("file2"));
}

#[test]
fn ls_a_shows_hidden() {
    let mut sh = shell();
    let out = feed(&mut sh, "ls -a /home/5cotts");
    assert!(out.contains(".hints"));
}

#[test]
fn ls_no_hidden_by_default() {
    let mut sh = shell();
    let out = feed(&mut sh, "ls /home/5cotts");
    assert!(!out.contains(".hints"));
}

#[test]
fn cp_file() {
    let mut sh = shell();
    feed(&mut sh, "touch /home/5cotts/orig.txt");
    feed(&mut sh, "cp /home/5cotts/orig.txt /home/5cotts/copy.txt");
    let out = feed(&mut sh, "ls /home/5cotts");
    assert!(out.contains("copy.txt"));
    assert!(out.contains("orig.txt"));
}

#[test]
fn mv_file() {
    let mut sh = shell();
    feed(&mut sh, "touch /home/5cotts/old.txt");
    feed(&mut sh, "mv /home/5cotts/old.txt /home/5cotts/new.txt");
    let out = feed(&mut sh, "ls /home/5cotts");
    assert!(out.contains("new.txt"));
    assert!(!out.contains("old.txt"));
}

#[test]
fn rm_file() {
    let mut sh = shell();
    feed(&mut sh, "touch /home/5cotts/del.txt");
    feed(&mut sh, "rm /home/5cotts/del.txt");
    let out = feed(&mut sh, "ls /home/5cotts");
    assert!(!out.contains("del.txt"));
}

#[test]
fn cat_nonexistent_fails() {
    let mut sh = shell();
    let out = feed(&mut sh, "cat /nonexistent_file");
    assert!(out.contains("No such file or directory"));
}
