//! Integration tests for external command execution via `NativeProcessHost`.
//! Gated on `native-proc` so they only run in the CLI test profile.

#![cfg(feature = "native-proc")]

use std::time::Instant;
use wat_core::io::{OutputSink, VecSink};
use wat_core::process::NativeProcessHost;
use wat_core::Shell;

fn native_shell() -> Shell {
    let mut sh = Shell::new().with_process_host(Box::new(NativeProcessHost));
    // Land on the host's actual cwd so spawned children have a real directory.
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
fn echo_via_bin_echo_writes_to_stdout() {
    let mut sh = native_shell();
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming("/bin/echo hello-from-bin-echo", &mut out, &mut err);
    assert_eq!(code, 0);
    let text = String::from_utf8_lossy(out.as_slice());
    assert!(text.contains("hello-from-bin-echo"), "got: {:?}", text);
    assert!(err.as_slice().is_empty(), "stderr should be empty");
}

#[test]
fn external_false_returns_exit_code_1() {
    let mut sh = native_shell();
    // Resolve `false` via PATH lookup so it goes through ProcessHost rather
    // than the wat builtin.
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming("/bin/false", &mut out, &mut err);
    assert_eq!(code, 1);
}

#[test]
fn unknown_external_emits_command_not_found_and_127() {
    let mut sh = native_shell();
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming("definitely-not-a-real-command-xyz", &mut out, &mut err);
    assert_eq!(code, 127);
    let err_text = String::from_utf8_lossy(err.as_slice());
    assert!(
        err_text.contains("command not found"),
        "stderr: {:?}",
        err_text
    );
}

/// Sink that timestamps each write so we can assert that output arrived in
/// distinct chunks separated by time, not all at the end.
#[derive(Default)]
struct TimedSink {
    writes: Vec<(std::time::Duration, Vec<u8>)>,
    start: Option<Instant>,
}

impl TimedSink {
    fn new() -> Self {
        Self {
            writes: Vec::new(),
            start: Some(Instant::now()),
        }
    }
}

impl OutputSink for TimedSink {
    fn write(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let t = self.start.unwrap_or_else(Instant::now);
        self.writes.push((t.elapsed(), chunk.to_vec()));
    }
}

#[test]
fn external_output_streams_live_not_just_at_end() {
    // A small shell script that prints "alpha", sleeps 250ms, prints "beta".
    // We expect to see the sink receive these in two distinct chunks
    // separated by ~250ms — proving streaming, not a single end-of-process
    // dump.
    let mut sh = native_shell();
    let mut out = TimedSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming(
        "sh -c 'printf alpha; sleep 0.25; printf beta'",
        &mut out,
        &mut err,
    );
    assert_eq!(
        code,
        0,
        "stderr: {:?}",
        String::from_utf8_lossy(err.as_slice())
    );

    let combined: Vec<u8> = out
        .writes
        .iter()
        .flat_map(|(_, b)| b.iter().copied())
        .collect();
    let text = String::from_utf8_lossy(&combined);
    assert!(text.contains("alpha"), "missing alpha in {:?}", text);
    assert!(text.contains("beta"), "missing beta in {:?}", text);

    // Locate the first chunk containing alpha vs beta and check their times.
    let alpha_t = out
        .writes
        .iter()
        .find(|(_, b)| b.windows(5).any(|w| w == b"alpha"))
        .map(|(t, _)| *t)
        .expect("alpha not found");
    let beta_t = out
        .writes
        .iter()
        .find(|(_, b)| b.windows(4).any(|w| w == b"beta"))
        .map(|(t, _)| *t)
        .expect("beta not found");

    let gap = beta_t.saturating_sub(alpha_t);
    assert!(
        gap.as_millis() >= 150,
        "expected ≥150ms gap between alpha and beta writes (got {:?}); proves output is not buffered to the end",
        gap
    );
}

#[test]
fn external_inherits_shell_cwd() {
    // /bin/pwd reports the current working directory. We set the shell's cwd
    // explicitly and confirm the spawned child sees it.
    let mut sh = native_shell();
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    sh.ctx.env.cwd = tmp.clone();
    sh.ctx.env.set("PWD", &tmp);
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming("/bin/pwd", &mut out, &mut err);
    assert_eq!(code, 0);
    let text = String::from_utf8_lossy(out.as_slice());
    // Some hosts symlink /tmp to /private/tmp (macOS) so accept a contains-match.
    assert!(
        text.trim().ends_with(
            tmp.trim_start_matches('/')
                .split('/')
                .next_back()
                .unwrap_or("tmp")
        ),
        "expected pwd to include shell cwd; got {:?} for cwd {:?}",
        text,
        tmp
    );
}
