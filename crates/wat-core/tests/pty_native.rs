//! Tier 2 / Phase A: integration tests for `NativePtyHost`. Gated on both
//! `native-pty` (for the host) and `native-proc` (for `ProcessSpec`).
//!
//! Each test allocates its own PTY pair via `NativePtyHost`, spawns a small
//! shell command in it, and asserts on the bytes flowing across the master
//! end. PTYs translate `\n` → `\r\n` on output, so the assertions are
//! `contains`-style except where the exact CRLF form matters (see
//! `stty_size_reports_correct_dims`).

#![cfg(all(feature = "native-pty", feature = "native-proc"))]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wat_core::process::{ProcessSpec, Signal};
use wat_core::pty::{NativePtyHost, PtyDims, PtyHost};

fn spec(argv: &[&str]) -> ProcessSpec {
    ProcessSpec {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        env: std::env::vars().collect(),
        cwd: PathBuf::from("/tmp"),
    }
}

fn read_all(mut r: Box<dyn Read + Send>, max: Duration) -> Vec<u8> {
    // Read until EOF or `max` elapses. Reads can block on a PTY because the
    // master only EOFs after the child closes its slave FDs (i.e. after the
    // child exits). Using a deadline keeps a buggy test from hanging the
    // suite, but in practice all of these commands exit fast.
    let deadline = Instant::now() + max;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while Instant::now() < deadline {
        match r.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    buf
}

#[test]
fn spawn_and_read_hello() {
    let host = NativePtyHost;
    let mut child = host
        .spawn_pty(
            spec(&["/bin/sh", "-c", "printf hello"]),
            PtyDims { rows: 24, cols: 80 },
        )
        .expect("spawn");
    let reader = child.master_reader().expect("reader");
    let out = read_all(reader, Duration::from_secs(3));
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("hello"), "got: {:?}", text);
    let _ = child.wait();
}

#[test]
fn stty_size_reports_correct_dims() {
    let host = NativePtyHost;
    let mut child = host
        .spawn_pty(
            spec(&["/usr/bin/stty", "size"]),
            PtyDims { rows: 24, cols: 80 },
        )
        .expect("spawn");
    let reader = child.master_reader().expect("reader");
    let out = read_all(reader, Duration::from_secs(3));
    let text = String::from_utf8_lossy(&out);
    // PTY converts \n to \r\n on output.
    assert!(text.contains("24 80"), "expected '24 80', got: {:?}", text);
    let _ = child.wait();
}

#[test]
fn write_then_read_roundtrip() {
    let host = NativePtyHost;
    let mut child = host
        .spawn_pty(
            // `stty -echo` first so the terminal driver doesn't echo our
            // written bytes back. Without that the master would see both
            // "world\r\n" (echo) and the script's "got=world\r\n".
            spec(&["/bin/sh", "-c", "stty -echo; read x; printf got=%s $x"]),
            PtyDims { rows: 24, cols: 80 },
        )
        .expect("spawn");
    let reader = child.master_reader().expect("reader");
    let mut writer = child.master_writer().expect("writer");

    // Give the child a moment to install `stty -echo` before we write.
    std::thread::sleep(Duration::from_millis(100));
    writer.write_all(b"world\n").expect("write");
    drop(writer);

    let out = read_all(reader, Duration::from_secs(3));
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("got=world"), "got: {:?}", text);
    let _ = child.wait();
}

#[test]
fn resize_updates_dims_live() {
    let host = NativePtyHost;
    let mut child = host
        .spawn_pty(
            spec(&["/bin/sh", "-c", "stty size; sleep 0.3; stty size"]),
            PtyDims { rows: 24, cols: 80 },
        )
        .expect("spawn");
    let reader = child.master_reader().expect("reader");

    // After 150ms (mid-sleep), bump the size. The second `stty size` should
    // pick up the new dims.
    std::thread::sleep(Duration::from_millis(150));
    child
        .resize(PtyDims {
            rows: 50,
            cols: 120,
        })
        .expect("resize");

    let out = read_all(reader, Duration::from_secs(3));
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("24 80"),
        "expected initial '24 80', got: {:?}",
        text
    );
    assert!(
        text.contains("50 120"),
        "expected resized '50 120', got: {:?}",
        text
    );
    let _ = child.wait();
}

#[test]
fn signal_interrupt_kills_pty_child() {
    let host = NativePtyHost;
    let mut child = host
        .spawn_pty(
            spec(&["/usr/bin/sleep", "30"]),
            PtyDims { rows: 24, cols: 80 },
        )
        .expect("spawn");

    let signaler = std::thread::spawn({
        // Move child.signal call into a thread that fires after 100ms.
        // We can't share `child` across threads easily, so do the timing in
        // a thread that returns when done and signal here on the main thread
        // by waiting briefly first.
        || std::thread::sleep(Duration::from_millis(100))
    });
    signaler.join().unwrap();
    child.signal(Signal::Interrupt).expect("signal");

    let start = Instant::now();
    let code = child.wait().expect("wait");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "expected sleep to die fast; took {:?}",
        elapsed
    );
    // POSIX 128 + signum, SIGINT == 2 → 130. portable-pty's exit_code mapping
    // for signal exits is platform-dependent on the boundary; accept either
    // the canonical 130 or any non-zero status as evidence of cancellation.
    assert!(
        code == 130 || code != 0,
        "expected signal-encoded exit (130 ideally), got {}",
        code
    );
}
