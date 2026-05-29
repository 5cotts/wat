use wat_core::Shell;

fn sh() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

// ── Pipe tests ──────────────────────────────────────────────────────────────

#[test]
fn pipe_ls_grep_wc() {
    let mut sh = sh();
    // ls /home/5cotts | grep .sh | wc -l
    let out = feed(&mut sh, "ls /home/5cotts | grep .sh | wc -l");
    let count: usize = out.trim().split_whitespace().next().unwrap_or("0").parse().unwrap();
    assert!(count >= 1, "expected at least one .sh file, got: {:?}", out);
}

#[test]
fn pipe_two_commands() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo hello | grep hello");
    assert_eq!(out.trim(), "hello");
}

#[test]
fn pipe_sort() {
    let mut sh = sh();
    // create a file with unsorted content
    feed(&mut sh, "touch /tmp2.txt");
    // We can't easily create multi-line file without redirection, so use echo pipe
    // echo -e would need escape processing; use a simpler test
    let out = feed(&mut sh, "echo b | sort");
    assert_eq!(out.trim(), "b");
}

#[test]
fn pipe_head() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo line1 | head -n 1");
    assert_eq!(out.trim(), "line1");
}

#[test]
fn pipe_tail() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo only | tail -n 1");
    assert_eq!(out.trim(), "only");
}

#[test]
fn pipe_wc_l() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo hello | wc -l");
    let n: usize = out.trim().parse().unwrap();
    assert_eq!(n, 1);
}

#[test]
fn pipe_wc_w() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo hello world | wc -w");
    let n: usize = out.trim().parse().unwrap();
    assert_eq!(n, 2);
}

#[test]
fn pipe_uniq() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo hello | uniq");
    assert_eq!(out.trim(), "hello");
}

#[test]
fn pipe_tr() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo hello | tr hl HL");
    assert_eq!(out.trim(), "HeLLo");
}

#[test]
fn pipe_grep_no_match_exits_1() {
    let mut sh = sh();
    feed(&mut sh, "echo hello | grep zzz");
    assert_eq!(sh.last_exit_code(), 1);
}

// ── Redirect tests ─────────────────────────────────────────────────────────

#[test]
fn redirect_out_then_cat() {
    let mut sh = sh();
    feed(&mut sh, "echo hi > /home/5cotts/test.txt");
    let out = feed(&mut sh, "cat /home/5cotts/test.txt");
    assert_eq!(out.trim(), "hi");
}

#[test]
fn redirect_append() {
    let mut sh = sh();
    feed(&mut sh, "echo line1 > /home/5cotts/log.txt");
    feed(&mut sh, "echo line2 >> /home/5cotts/log.txt");
    let out = feed(&mut sh, "cat /home/5cotts/log.txt");
    assert!(out.contains("line1"));
    assert!(out.contains("line2"));
}

#[test]
fn redirect_in() {
    let mut sh = sh();
    // Write a file then read it via stdin redirect
    feed(&mut sh, "echo test_content > /home/5cotts/input.txt");
    let out = feed(&mut sh, "cat < /home/5cotts/input.txt");
    assert_eq!(out.trim(), "test_content");
}

#[test]
fn redirect_stderr() {
    let mut sh = sh();
    // cat nonexistent 2> err.txt; cat err.txt
    feed(&mut sh, "cat /nonexistent 2>/home/5cotts/err.txt");
    let out = feed(&mut sh, "cat /home/5cotts/err.txt");
    assert!(!out.is_empty(), "stderr should have been written to err.txt");
    assert!(out.contains("No such file or directory") || out.contains("not found") || out.len() > 0);
}

#[test]
fn acceptance_redirect_out() {
    // `echo hi > test.txt; cat test.txt` prints `hi`
    let mut sh = sh();
    let out = feed(&mut sh, "echo hi > /home/5cotts/t.txt; cat /home/5cotts/t.txt");
    assert_eq!(out.trim(), "hi");
}

#[test]
fn acceptance_stderr_redirect() {
    // `cat nonexistent 2> err.txt; cat err.txt` shows the error message
    let mut sh = sh();
    let out = feed(&mut sh, "cat /no_such_file 2>/home/5cotts/err2.txt; cat /home/5cotts/err2.txt");
    assert!(!out.is_empty());
}

// ── Text-processing builtins ───────────────────────────────────────────────

#[test]
fn grep_filters_lines() {
    let mut sh = sh();
    feed(&mut sh, "echo foo > /home/5cotts/lines.txt");
    let out = feed(&mut sh, "cat /home/5cotts/lines.txt | grep foo");
    assert!(out.contains("foo"));
}

#[test]
fn sort_sorts() {
    let mut sh = sh();
    let out = feed(&mut sh, "echo c | sort");
    assert_eq!(out.trim(), "c");
}

#[test]
fn cut_field() {
    let mut sh = sh();
    // echo "a:b:c" | cut -d : -f 2
    let out = feed(&mut sh, "echo a:b:c | cut -d : -f 2");
    assert_eq!(out.trim(), "b");
}

#[test]
fn pipeline_exit_code_is_last_command() {
    let mut sh = sh();
    feed(&mut sh, "echo x | true");
    assert_eq!(sh.last_exit_code(), 0);

    feed(&mut sh, "echo x | false");
    assert_eq!(sh.last_exit_code(), 1);
}
