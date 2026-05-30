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
