//! Tier 1 / Phase E: verifies SIGINT cancellation of a foreground child.
//! Instead of trying to deliver a real SIGINT to the test process (which
//! would also terminate the test runner), the test flips the same
//! `Arc<AtomicBool>` that `signal-hook` would flip and asserts the pipeline
//! tears down quickly with the expected signal-encoded exit code.

#![cfg(feature = "native-proc")]

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};
use wat_core::io::VecSink;
use wat_core::process::NativeProcessHost;
use wat_core::Shell;

fn native_shell() -> Shell {
    let mut sh = Shell::new().with_process_host(Box::new(NativeProcessHost));
    if let Ok(cwd) = std::env::current_dir() {
        let s = cwd.to_string_lossy().into_owned();
        sh.ctx.env.cwd = s.clone();
        sh.ctx.env.set("PWD", s);
    }
    if let Ok(path) = std::env::var("PATH") {
        sh.ctx.env.set("PATH", path);
    }
    sh
}

#[test]
fn sigint_cancels_long_running_child() {
    let mut sh = native_shell();
    let cancel = sh.cancel_flag();
    assert!(!cancel.load(Ordering::Relaxed));

    // After 100ms, flip the flag — mimics what `signal-hook` does inside the
    // signal handler when the user presses Ctrl-C.
    let flag = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        flag.store(true, Ordering::Relaxed);
    });

    let start = Instant::now();
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming("/usr/bin/sleep 30", &mut out, &mut err);
    let elapsed = start.elapsed();

    // The pipeline executor polls the cancel flag every 100ms; combined with
    // signal delivery + child reap, a tear-down well under 2s is a strong
    // signal that we actually interrupted instead of waited for `sleep 30`.
    assert!(
        elapsed < Duration::from_secs(2),
        "expected sleep to be cancelled quickly; took {:?}",
        elapsed
    );
    // POSIX: 128 + signum. SIGINT == 2 → exit code 130.
    assert_eq!(
        code,
        130,
        "expected SIGINT-encoded exit (130), got {} (stderr: {:?})",
        code,
        String::from_utf8_lossy(err.as_slice())
    );
}

#[test]
fn cancel_flag_does_not_affect_already_finished_commands() {
    // Pre-set the cancel flag, then run a quick command. The command should
    // still complete normally; the next command sees the flag we set, but
    // since there's no child to signal it has no effect either.
    let mut sh = native_shell();
    sh.cancel_flag().store(true, Ordering::Relaxed);

    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming("/bin/echo hello", &mut out, &mut err);
    assert_eq!(code, 0);
    let text = String::from_utf8_lossy(out.as_slice());
    assert!(text.contains("hello"), "got: {:?}", text);
}
