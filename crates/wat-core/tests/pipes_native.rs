//! Integration tests for cross-type pipelines (builtin↔external).
//! Gated on `native-proc` so they only run when wat-cli's feature set is on.

#![cfg(feature = "native-proc")]

use std::time::Instant;
use wat_core::io::{OutputSink, VecSink};
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

fn feed(sh: &mut Shell, input: &str) -> (i32, String, String) {
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming(input, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(out.as_slice()).into_owned(),
        String::from_utf8_lossy(err.as_slice()).into_owned(),
    )
}

#[test]
fn builtin_to_builtin_pipeline() {
    // `echo` is a wat builtin and `wc` is a wat builtin.
    let mut sh = native_shell();
    let (code, out, _err) = feed(&mut sh, "echo hello world | wc -w");
    assert_eq!(code, 0);
    assert!(out.trim().starts_with('2'), "got: {:?}", out);
}

#[test]
fn builtin_to_external_pipeline() {
    // `echo` is a wat builtin; `/usr/bin/rev` is external.
    let mut sh = native_shell();
    let (code, out, _err) = feed(&mut sh, "echo hello | rev");
    assert_eq!(code, 0);
    assert!(out.contains("olleh"), "got: {:?}", out);
}

#[test]
fn external_to_builtin_pipeline() {
    // `/bin/echo` is external; `wc` is a wat builtin.
    let mut sh = native_shell();
    let (code, out, _err) = feed(&mut sh, "/bin/echo line1 line2 line3 | wc -w");
    assert_eq!(code, 0);
    assert!(out.trim().starts_with('3'), "got: {:?}", out);
}

#[test]
fn external_to_external_pipeline() {
    // Both external: `/bin/echo` then `/usr/bin/rev`.
    let mut sh = native_shell();
    let (code, out, _err) = feed(&mut sh, "/bin/echo foobar | /usr/bin/rev");
    assert_eq!(code, 0);
    assert!(out.contains("raboof"), "got: {:?}", out);
}

#[test]
fn three_segment_mixed_pipeline() {
    // builtin | external | builtin
    let mut sh = native_shell();
    let (code, out, _err) = feed(&mut sh, "echo abc def ghi | /usr/bin/rev | wc -w");
    assert_eq!(code, 0);
    assert!(out.trim().starts_with('3'), "got: {:?}", out);
}

#[test]
fn exit_status_from_last_segment() {
    // `true | false` should report 1 (POSIX behavior — last command's exit
    // status propagates).
    let mut sh = native_shell();
    let (code, _, _) = feed(&mut sh, "echo ignored | /bin/false");
    assert_eq!(code, 1);
}

/// Sink that timestamps each write so we can assert that pipeline output
/// arrived in distinct chunks separated by time, not as a single blob.
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
fn external_pipeline_uses_os_pipes_not_buffering() {
    // The strongest proof that we're not buffering the upstream in the
    // parent: `yes | head -1` MUST terminate fast — `head` closes its stdin
    // after one line, which propagates SIGPIPE back to `yes` via the OS
    // pipe (or, in our case, via the writer thread dropping its reader).
    // If we were draining `yes` into a parent-side VecSink we'd run forever.
    // Note: both segments must be external — wat's builtin `head` reads its
    // whole stdin to a String, which would hang here.
    let mut sh = native_shell();
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let start = Instant::now();
    let code = sh.feed_streaming("/usr/bin/yes | /usr/bin/head -1", &mut out, &mut err);
    let elapsed = start.elapsed();

    // yes terminates from SIGPIPE → exit code 141 (128 + 13). head exits 0.
    // Last segment's exit status propagates → 0.
    assert_eq!(
        code,
        0,
        "exit code; stderr: {:?}",
        String::from_utf8_lossy(err.as_slice())
    );
    assert!(
        elapsed.as_millis() < 2000,
        "pipeline should finish in well under 2s — was {:?} (suggests the parent is buffering yes)",
        elapsed
    );
    let text = String::from_utf8_lossy(out.as_slice());
    assert_eq!(
        text.trim(),
        "y",
        "head -1 should produce one `y`; got {:?}",
        text
    );
}

#[test]
fn external_pipeline_streams_live() {
    // `cat` reads from stdin and writes to stdout with no stdio buffering
    // (uses read(2)/write(2) directly), so chunks pass through with the
    // upstream's natural timing. Avoids mawk's quirky `fflush()` behavior.
    let mut sh = native_shell();
    let mut out = TimedSink::new();
    let mut err = VecSink::new();
    let code = sh.feed_streaming(
        "sh -c 'printf \"one\\n\"; sleep 0.25; printf \"two\\n\"' | /bin/cat",
        &mut out,
        &mut err,
    );
    assert_eq!(
        code,
        0,
        "stderr: {:?}",
        String::from_utf8_lossy(err.as_slice())
    );

    let one_t = out
        .writes
        .iter()
        .find(|(_, b)| b.windows(3).any(|w| w == b"one"))
        .map(|(t, _)| *t)
        .expect("one not found");
    let two_t = out
        .writes
        .iter()
        .find(|(_, b)| b.windows(3).any(|w| w == b"two"))
        .map(|(t, _)| *t)
        .expect("two not found");
    let gap = two_t.saturating_sub(one_t);
    assert!(
        gap.as_millis() >= 150,
        "expected ≥150ms gap between `one` and `two` chunks through the pipe (got {:?})",
        gap
    );
}

#[test]
fn pipeline_with_unknown_external_continues_with_empty_stdin() {
    // POSIX behavior: when an early segment fails to launch, subsequent
    // segments still run with empty stdin.
    let mut sh = native_shell();
    let (code, out, err) = feed(&mut sh, "definitely-not-real-xyz | wc -l");
    // wc -l on empty input should print 0.
    assert!(
        err.contains("command not found"),
        "expected command-not-found in stderr; got: {:?}",
        err
    );
    assert!(
        out.trim().starts_with('0'),
        "expected wc -l of empty input == 0; got out: {:?}",
        out
    );
    // The last segment is wc, which succeeds; exit code from it is 0.
    assert_eq!(code, 0);
}
