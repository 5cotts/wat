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

// ── Tier 3 / Phase A: drive-loop regression ──────────────────────────────

#[test]
fn pty_normal_command_still_works_after_drive_loop_refactor() {
    // Smoke test: the SIGCHLD-aware mpsc drive loop still forwards output
    // and returns to the prompt for a plain command.
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer
        .write_all(b"python3 -c 'print(\"hello\")'\n")
        .expect("write");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains("hello"),
        "expected 'hello' from PTY command, got: {:?}",
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

// ── Tier 3 / Phase B: Ctrl-Z stops child, returns to prompt ──────────────

#[test]
fn ctrl_z_stops_pty_child_and_returns_to_prompt() {
    let (_master, mut reader, mut writer, mut child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    // sleep 30 is a single external command → PTY-routed.
    writer.write_all(b"sleep 30\n").expect("write sleep");
    // Give the child a moment to start running inside the PTY.
    std::thread::sleep(Duration::from_millis(300));

    // Send Ctrl-Z (0x1a). The PTY driver forwards it to the inner slave as
    // SIGTSTP; sleep stops. wat-cli detects via try_wait and returns to prompt.
    writer.write_all(b"\x1a").expect("write ctrl-z");

    // Read until we see a prompt again.
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains("Stopped"),
        "expected 'Stopped' notification after Ctrl-Z, got: {:?}",
        after
    );

    // wat-cli should still be alive and responsive. There's a stopped job, so
    // the first `exit` warns and is cancelled; a second consecutive `exit`
    // quits.
    writer.write_all(b"exit\nexit\n").expect("exit");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().ok();
            panic!("wat did not exit after ctrl-z test");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ── Tier 3 / Phase C: jobs / fg / bg builtins ────────────────────────────

fn ctrl_z_sleep(writer: &mut Box<dyn Write + Send>, _reader: &mut Box<dyn Read + Send>) {
    writer.write_all(b"sleep 30\n").expect("write sleep");
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b"\x1a").expect("ctrl-z");
}

fn wait_for_wat_exit(mut child: Box<dyn portable_pty::Child + Send + Sync>) {
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return;
        }
        if start.elapsed() > Duration::from_secs(8) {
            child.kill().ok();
            panic!("wat did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn jobs_lists_stopped_job() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    ctrl_z_sleep(&mut writer, &mut reader);
    let stopped_out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        stopped_out.contains("Stopped"),
        "expected Stopped message, got: {:?}",
        stopped_out
    );

    writer.write_all(b"jobs\n").expect("write jobs");
    let jobs_out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        jobs_out.contains("Stopped"),
        "expected 'Stopped' in jobs output, got: {:?}",
        jobs_out
    );
    assert!(
        jobs_out.contains("sleep"),
        "expected 'sleep' in jobs output, got: {:?}",
        jobs_out
    );

    // Stopped job present → first exit warns; second quits.
    writer.write_all(b"exit\nexit\n").expect("exit");
    wait_for_wat_exit(child);
}

#[test]
fn fg_resumes_stopped_job_in_foreground() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    // Stop a short sleep (0.5s) with Ctrl-Z.
    writer.write_all(b"sleep 0.5\n").expect("write sleep");
    std::thread::sleep(Duration::from_millis(150));
    writer.write_all(b"\x1a").expect("ctrl-z");

    let after_stop = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after_stop.contains("Stopped"),
        "expected Stopped, got: {:?}",
        after_stop
    );

    // fg resumes sleep 0.5; it should run to completion and return to prompt.
    writer.write_all(b"fg\n").expect("write fg");
    let after_fg = read_until(&mut reader, PROMPT_MARKER, deadline());

    // Positive evidence fg actually re-drove the job: it must NOT error out
    // ("no writer", "no such job") and must NOT re-stop on its own.
    assert!(
        !after_fg.contains("no writer") && !after_fg.contains("no such job"),
        "fg errored instead of resuming: {:?}",
        after_fg
    );
    assert!(
        !after_fg.contains("Stopped"),
        "expected sleep to finish, not stop again, got: {:?}",
        after_fg
    );

    // Strongest check: the shell is responsive afterward. If fg had hung in
    // the drive loop, this marker command would time out.
    writer
        .write_all(b"echo resumed_ok\n")
        .expect("write marker");
    let after_marker = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after_marker.contains("resumed_ok"),
        "shell not responsive after fg; got: {:?}",
        after_marker
    );

    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}

#[test]
fn fg_then_ctrl_z_restops_then_responsive() {
    // Regression for the clone_writer fix. A job must survive
    // Ctrl-Z → fg → Ctrl-Z and leave the shell responsive. The original
    // take_writer's EOF-on-drop corrupted the inner tty so the SECOND Ctrl-Z
    // never reached the child and the drive loop hung forever.
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer.write_all(b"sleep 30\n").expect("write sleep");
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b"\x1a").expect("ctrl-z 1");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(out.contains("Stopped"), "first stop failed: {:?}", out);

    writer.write_all(b"fg\n").expect("write fg");
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b"\x1a").expect("ctrl-z 2");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("Stopped"),
        "second Ctrl-Z after fg did not re-stop (drive loop hang?): {:?}",
        out
    );

    writer
        .write_all(b"echo still_alive\n")
        .expect("write marker");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("still_alive"),
        "shell hung after fg/ctrl-z cycle: {:?}",
        out
    );

    // Job still stopped → first exit warns; second quits.
    writer.write_all(b"exit\nexit\n").expect("exit");
    wait_for_wat_exit(child);
}

#[test]
fn bg_resumes_stopped_job_in_background() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    ctrl_z_sleep(&mut writer, &mut reader);
    let after_stop = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after_stop.contains("Stopped"),
        "expected Stopped, got: {:?}",
        after_stop
    );

    // bg returns immediately and acknowledges by naming the resumed job.
    let bg_start = Instant::now();
    writer.write_all(b"bg\n").expect("write bg");
    let after_bg = read_until(&mut reader, PROMPT_MARKER, deadline());
    let bg_elapsed = bg_start.elapsed();
    assert!(
        bg_elapsed < Duration::from_secs(3),
        "bg should return quickly, took {:?}",
        bg_elapsed
    );
    assert!(
        after_bg.contains("continued") && after_bg.contains("sleep"),
        "bg did not acknowledge resuming the job: {:?}",
        after_bg
    );

    // The backgrounded job is `sleep 30`, so it MUST still be Running when we
    // check — no escape hatch for "maybe it finished".
    writer.write_all(b"jobs\n").expect("write jobs");
    let jobs_out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        jobs_out.contains("Running") && jobs_out.contains("sleep"),
        "expected a Running sleep job after bg, got: {:?}",
        jobs_out
    );

    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}

// ── Tier 3 / Phase D: & background spawn + Done notifications ────────────

#[test]
fn ampersand_runs_in_background_and_returns_to_prompt() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    // sleep 0.3 & should return to prompt almost immediately and register a
    // job (spawn prints `[N] <pid>`).
    let start = Instant::now();
    writer.write_all(b"sleep 0.3 &\n").expect("write bg cmd");
    let after_bg = read_until(&mut reader, PROMPT_MARKER, deadline());
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "background command took too long ({:?}); should return immediately",
        elapsed
    );
    assert!(
        after_bg.contains("[1]"),
        "expected background job registration '[1] <pid>', got: {:?}",
        after_bg
    );

    // Type a mid-execution command while sleep is still running.
    writer.write_all(b"echo middle\n").expect("write middle");
    let after_echo = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after_echo.contains("middle"),
        "expected 'middle' in output, got: {:?}",
        after_echo
    );

    // After 500ms, sleep 0.3 has finished. The next prompt MUST carry the
    // Done notification — no bare-bracket escape hatch.
    std::thread::sleep(Duration::from_millis(500));
    writer.write_all(b"echo end\n").expect("write end");
    let after_end = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after_end.contains("end"),
        "expected 'end' in output, got: {:?}",
        after_end
    );
    assert!(
        after_end.contains("Done"),
        "expected Done notification for finished bg job, got: {:?}",
        after_end
    );

    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}

#[test]
fn done_notification_uses_exit_status() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    writer
        .write_all(b"python3 -c 'import sys; sys.exit(3)' &\n")
        .expect("write bg py");
    read_until(&mut reader, PROMPT_MARKER, deadline());

    // Give python time to finish.
    std::thread::sleep(Duration::from_millis(500));

    writer.write_all(b"echo poke\n").expect("write poke");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    // The non-zero exit must surface as the exact `Exit 3` phrasing — a bare
    // "3" would match incidental output and mask a broken exit-status path.
    assert!(
        after.contains("Exit 3"),
        "expected 'Exit 3' notification, got: {:?}",
        after
    );

    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}

// ── Tier 3: full 'Done definition' e2e walkthrough in one session ─────────
//
// Mirrors the manual acceptance scenario from wat-tier3-plan.md's "Done
// definition": sleep 30 → Ctrl-Z → jobs → fg → Ctrl-Z → bg → sleep 0.3 & →
// Done. Runs against the built `wat` binary in a real PTY — the closest
// automated equivalent to a human driving the shell. This is the test that
// would have caught the fg/clone_writer regression that the per-feature
// tests' weak assertions missed.

#[test]
fn e2e_full_job_control_session() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    // Initial prompt.
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(out.contains(PROMPT_MARKER), "no initial prompt: {:?}", out);

    // 1. sleep 30 + Ctrl-Z → Stopped.
    writer.write_all(b"sleep 30\n").expect("sleep");
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b"\x1a").expect("ctrl-z");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(out.contains("Stopped"), "step1 no Stopped: {:?}", out);

    // 2. jobs lists it.
    writer.write_all(b"jobs\n").expect("jobs");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("Stopped") && out.contains("sleep"),
        "step2 jobs missing entry: {:?}",
        out
    );

    // 3. fg resumes, then Ctrl-Z re-stops.
    writer.write_all(b"fg\n").expect("fg");
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b"\x1a").expect("ctrl-z 2");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("Stopped"),
        "step3 fg+ctrlz no re-stop: {:?}",
        out
    );

    // 4. bg resumes in background, returns to prompt promptly.
    writer.write_all(b"bg\n").expect("bg");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(out.contains(PROMPT_MARKER), "step4 bg no prompt: {:?}", out);

    // 5. sleep 0.3 & backgrounds; Done shows at a later prompt.
    writer.write_all(b"sleep 0.3 &\n").expect("amp");
    read_until(&mut reader, PROMPT_MARKER, deadline());
    std::thread::sleep(Duration::from_millis(600));
    writer.write_all(b"echo done_check\n").expect("poke");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("done_check"),
        "step5 shell unresponsive: {:?}",
        out
    );
    assert!(
        out.contains("Done"),
        "step5 no Done notification for finished bg job: {:?}",
        out
    );

    // 6. clean exit (a kill of the still-stopped sleep 30 job is acceptable).
    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}

// ── Tier 3 follow-up: kill %N + "you have stopped jobs" exit warning ──────

#[test]
fn kill_percent_terminates_background_job() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    // Background a long sleep, then kill it by job spec.
    writer.write_all(b"sleep 30 &\n").expect("bg");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(out.contains("[1]"), "no job registration: {:?}", out);

    writer.write_all(b"kill %1\n").expect("kill");
    read_until(&mut reader, PROMPT_MARKER, deadline());

    // SIGTERM kills the sleep; the SIGCHLD handler marks it Done and the next
    // prompt carries the notification (signal exit → 128+15 = 143).
    std::thread::sleep(Duration::from_millis(200));
    writer.write_all(b"echo poke\n").expect("poke");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains("Exit") || after.contains("Done"),
        "expected a termination notification after kill, got: {:?}",
        after
    );

    // The job table is now empty.
    writer.write_all(b"jobs\n").expect("jobs");
    let jobs_out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        !jobs_out.contains("Running") && !jobs_out.contains("sleep"),
        "killed job should be gone from jobs, got: {:?}",
        jobs_out
    );

    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}

#[test]
fn exit_warns_about_stopped_jobs_then_quits() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    // Stop a job.
    writer.write_all(b"sleep 30\n").expect("sleep");
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b"\x1a").expect("ctrl-z");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(out.contains("Stopped"), "no stop: {:?}", out);

    // First exit warns and is cancelled — shell stays alive.
    writer.write_all(b"exit\n").expect("exit 1");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("You have stopped jobs"),
        "first exit should warn, got: {:?}",
        out
    );

    // A non-exit command resets the warning and proves the shell is alive.
    writer.write_all(b"echo alive\n").expect("echo");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("alive"),
        "shell not alive after warn: {:?}",
        out
    );

    // Because the warning reset, the next exit warns again rather than quitting.
    writer.write_all(b"exit\n").expect("exit 2");
    let out = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        out.contains("You have stopped jobs"),
        "exit after a command should warn again, got: {:?}",
        out
    );

    // A second *consecutive* exit proceeds.
    writer.write_all(b"exit\n").expect("exit 3");
    wait_for_wat_exit(child);
}

// ── Tier 5 / Phase C: an infinite loop is interruptible with Ctrl-C ───────

#[test]
fn ctrl_c_interrupts_infinite_loop() {
    let (_master, mut reader, mut writer, child) = spawn_wat_in_pty();
    let deadline = || Instant::now() + READ_TIMEOUT;

    read_until(&mut reader, PROMPT_MARKER, deadline());

    // A compound command isn't PTY-routed; it runs on the buffered path. The
    // loop checks the SIGINT cancel flag between iterations.
    writer
        .write_all(b"while true; do echo loop; done\n")
        .expect("write loop");
    // Wait until the loop is clearly running (some output produced).
    let started = read_until(&mut reader, "loop", deadline());
    assert!(
        started.contains("loop"),
        "loop did not start: {:?}",
        started
    );

    // Ctrl-C (0x03) → terminal sends SIGINT → handler sets cancel → loop breaks.
    std::thread::sleep(Duration::from_millis(100));
    writer.write_all(b"\x03").expect("write ctrl-c");

    // The shell returns to a prompt and is responsive again.
    read_until(&mut reader, PROMPT_MARKER, deadline());
    writer.write_all(b"echo recovered\n").expect("write marker");
    let after = read_until(&mut reader, PROMPT_MARKER, deadline());
    assert!(
        after.contains("recovered"),
        "shell not responsive after Ctrl-C of loop: {:?}",
        after
    );

    writer.write_all(b"exit\n").expect("exit");
    wait_for_wat_exit(child);
}
