//! Tier 2 / Phase B integration tests for the PTY-driven REPL.
//!
//! These tests spawn the actual `wat` binary inside a fresh PTY pair via
//! `portable-pty`, drive it by writing bytes to the master, and assert on
//! what comes back. This is the closest we can get in CI to "type a
//! command in a real terminal."
//!
//! Tests use `python3` because it's in our allowlist, exits cleanly, and
//! is installed on every reasonable Linux test host. Each test has a
//! hard timeout so a buggy regression can't hang the suite.
//!
//! The test always compiles — `portable-pty` is a dev-dep of `wat-cli` and
//! the production binary already pulls it in transitively via
//! `wat-core/native-pty`, so there's no value in feature-gating here.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const PROMPT_MARKER: &str = "% ";
const READ_TIMEOUT: Duration = Duration::from_secs(8);

/// Read from the master until we see `marker` in the accumulated output,
/// or until `deadline` passes. Returns everything read so far. Used to
/// resync with the REPL after each command.
fn read_until(reader: &mut Box<dyn Read + Send>, marker: &str, deadline: Instant) -> String {
    let mut accumulated = Vec::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                accumulated.extend_from_slice(&buf[..n]);
                if std::str::from_utf8(&accumulated)
                    .map(|s| s.contains(marker))
                    .unwrap_or(false)
                {
                    return String::from_utf8_lossy(&accumulated).into_owned();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&accumulated).into_owned()
}

fn spawn_wat_in_pty() -> (
    Box<dyn portable_pty::MasterPty + Send>,
    Box<dyn Read + Send>,
    Box<dyn Write + Send>,
    Box<dyn portable_pty::Child + Send + Sync>,
) {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let bin = env!("CARGO_BIN_EXE_wat");
    let mut cmd = CommandBuilder::new(bin);
    // Land in /tmp so cwd-derived prompt text is short and stable.
    cmd.cwd("/tmp");
    // Inherit PATH so the child shell can find /usr/bin/python3 etc.
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    if let Ok(home) = std::env::var("HOME") {
        cmd.env("HOME", home);
    }
    cmd.env("TERM", "xterm-256color");
    let child = pair.slave.spawn_command(cmd).expect("spawn wat");
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().expect("clone reader");
    let writer = pair.master.take_writer().expect("take writer");
    (pair.master, reader, writer, child)
}

#[test]
fn repl_runs_pty_command_and_returns_to_prompt() {
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();

    let deadline = || Instant::now() + READ_TIMEOUT;

    // First prompt.
    let initial = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        initial.contains(PROMPT_MARKER),
        "no initial prompt; got: {:?}",
        initial
    );

    // PTY-routed command: python3 is on the allowlist.
    writer
        .write_all(b"python3 -c 'print(\"pty-ok\")'\n")
        .expect("write");

    // Read until the next prompt appears.
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains("pty-ok"),
        "expected 'pty-ok' in output, got: {:?}",
        after
    );

    writer.write_all(b"exit\n").expect("exit");

    // Reap with a deadline; if the child hangs past 5s, fail.
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            assert_eq!(status.exit_code(), 0, "wat exited non-zero");
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit within 5s after `exit`");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn repl_can_run_normal_command_after_pty_command() {
    // Verifies the RawModeGuard drop restores cooked mode so the next
    // command goes through `feed_streaming` normally.
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer
        .write_all(b"python3 -c 'print(\"first\")'\n")
        .expect("write 1");
    let after1 = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(after1.contains("first"), "got: {:?}", after1);

    writer.write_all(b"echo second\n").expect("write 2");
    let after2 = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(after2.contains("second"), "got: {:?}", after2);

    writer.write_all(b"exit\n").expect("exit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn resize_propagates_to_running_pty_child() {
    // Spawn wat in an 24x80 PTY. Run a python process that polls
    // `os.get_terminal_size()` once per 100ms for ~2s and prints whenever
    // the value changes. After 300ms (long enough for python to install
    // its handler and start polling), resize the master to 50x120. The
    // child should observe the new size — wat's SIGWINCH handler turns
    // around and calls `child.resize(...)`, which sets the slave PTY's
    // winsize, and python's poll picks it up on its next iteration.
    //
    // Using polling instead of signal.signal(SIGWINCH, ...) because the
    // SIGWINCH-handler approach is racy across Python implementations
    // and the test just needs to prove that the slave's winsize changes.
    let (master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    let py = "python3 -c \"import os, sys, time\\nlast = None\\nfor _ in range(20):\\n  s = os.get_terminal_size()\\n  if s != last:\\n    print(f'size={s.columns}x{s.lines}', flush=True)\\n    last = s\\n  time.sleep(0.1)\"\n";
    writer.write_all(py.as_bytes()).expect("write py");

    // Give python a moment to start polling and print its initial size.
    std::thread::sleep(Duration::from_millis(300));
    // Update the outer master's window size so crossterm inside wat-cli
    // reads back the new dimensions when its SIGWINCH handler fires.
    master
        .resize(PtySize {
            rows: 50,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("resize master");
    // In a real terminal, TIOCSWINSZ on the master automatically delivers
    // SIGWINCH to the controlling pgrp of the slave (xterm/iterm/etc.
    // depend on this). portable-pty's test-process-as-master setup
    // doesn't reliably trigger that delivery here, so send the signal
    // explicitly. wat-cli's handler then reads the (already updated) dims
    // via crossterm and forwards them to the inner PTY where python is
    // running — the same chain that runs in production.
    if let Some(pid) = child.process_id() {
        unsafe {
            extern "C" {
                fn kill(pid: i32, sig: i32) -> i32;
            }
            kill(pid as i32, 28); // SIGWINCH == 28 on Linux
        }
    }

    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains("size=80x24"),
        "expected initial size=80x24, got: {:?}",
        after
    );
    assert!(
        after.contains("size=120x50"),
        "expected resized size=120x50, got: {:?}",
        after
    );

    writer.write_all(b"exit\n").expect("exit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn pipelines_still_use_piped_path() {
    // Multiple segments → not PTY-eligible. Must go through the buffered
    // streaming path and produce the captured wc -w result.
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;
    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer.write_all(b"echo a b c | wc -w\n").expect("write");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    // wc -w prints "3" (possibly with leading whitespace).
    let body = after
        .lines()
        .find(|l| l.trim() == "3" || l.trim().starts_with('3'))
        .unwrap_or("");
    assert!(
        body.trim() == "3" || body.trim().starts_with('3'),
        "expected '3' from `echo a b c | wc -w`, got: {:?}",
        after
    );

    writer.write_all(b"exit\n").expect("exit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn redirects_still_use_piped_path() {
    // Output redirect → must NOT use PTY. Write hello to /tmp/<unique>,
    // then cat it back to verify the byte landed via the piped path.
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;
    read_until(&mut reader, PROMPT_MARKER, deadline());

    let path = format!("/tmp/wat-tier2-d-redirect-{}", std::process::id());
    // Use the wat builtin `echo` (which always goes through the piped
    // path) so we don't depend on /bin/echo being on PATH inside the
    // shell's tracked env. The interesting bit is the `>` operator.
    let cmd = format!("echo hello > {}\n", path);
    writer.write_all(cmd.as_bytes()).expect("write 1");
    read_until(&mut reader, PROMPT_MARKER, deadline());

    let read_cmd = format!("/bin/cat {}\n", path);
    writer.write_all(read_cmd.as_bytes()).expect("write 2");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(after.contains("hello"), "got: {:?}", after);

    writer.write_all(b"exit\n").expect("exit");
    let _ = std::fs::remove_file(&path);
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn pty_command_exit_code_propagates() {
    // The PTY child's exit code must end up in $?.
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;
    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer
        .write_all(b"python3 -c 'import sys; sys.exit(7)'\n")
        .expect("write 1");
    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer.write_all(b"echo $?\n").expect("write 2");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains('7'),
        "expected exit code 7 to surface via $?, got: {:?}",
        after
    );

    writer.write_all(b"exit\n").expect("exit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn repl_pty_command_nonzero_exit_does_not_hang_repl() {
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer
        .write_all(b"python3 -c 'import sys; sys.exit(7)'\n")
        .expect("write 1");
    // We don't assert on output here — Python with sys.exit doesn't print
    // anything. The important thing is that we get back to a prompt.
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(after.contains(PROMPT_MARKER), "no prompt; got: {:?}", after);

    writer.write_all(b"echo recovered\n").expect("write 2");
    let after2 = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(after2.contains("recovered"), "got: {:?}", after2);

    writer.write_all(b"exit\n").expect("exit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
