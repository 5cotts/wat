use std::cell::RefCell;
use std::rc::Rc;
use wat_core::io::{OutputSink, VecSink};
use wat_core::Shell;

/// A sink that records each `write` call as a separate entry, so tests can
/// assert that output arrived in distinct chunks rather than one combined
/// blob at the end.
#[derive(Clone, Default)]
struct ChunkSink {
    chunks: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl ChunkSink {
    fn new() -> Self {
        Self::default()
    }

    fn chunks(&self) -> Vec<Vec<u8>> {
        self.chunks.borrow().clone()
    }

    fn joined(&self) -> String {
        let mut out = Vec::new();
        for c in self.chunks.borrow().iter() {
            out.extend_from_slice(c);
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

impl OutputSink for ChunkSink {
    fn write(&mut self, chunk: &[u8]) {
        if !chunk.is_empty() {
            self.chunks.borrow_mut().push(chunk.to_vec());
        }
    }
}

#[test]
fn feed_and_feed_streaming_produce_identical_output() {
    let mut buffered = Shell::with_memory_vfs();
    let buffered_out = buffered.feed("echo hello world");

    let mut streamed = Shell::with_memory_vfs();
    let mut sink = VecSink::new();
    let mut err = VecSink::new();
    let code = streamed.feed_streaming("echo hello world", &mut sink, &mut err);

    assert_eq!(code, 0);
    let mut combined = sink.into_inner();
    combined.extend_from_slice(err.as_slice());
    assert_eq!(buffered_out, String::from_utf8_lossy(&combined));
    assert!(buffered_out.contains("hello world"));
}

#[test]
fn cat_with_multiple_files_streams_per_file() {
    let mut sh = Shell::with_memory_vfs();
    // Seed the VFS with two files.
    sh.feed("echo aaa > /home/5cotts/a.txt");
    sh.feed("echo bbb > /home/5cotts/b.txt");

    let sink = ChunkSink::new();
    let mut sink_handle = sink.clone();
    let mut err = VecSink::new();
    let code = sh.feed_streaming(
        "cat /home/5cotts/a.txt /home/5cotts/b.txt",
        &mut sink_handle,
        &mut err,
    );

    assert_eq!(code, 0, "cat should succeed");
    let chunks = sink.chunks();
    assert!(
        chunks.len() >= 2,
        "cat of two files should produce at least two sink writes (one per file), got {:?}",
        chunks
    );
    let joined = sink.joined();
    assert!(joined.contains("aaa"));
    assert!(joined.contains("bbb"));
    assert!(joined.find("aaa").unwrap() < joined.find("bbb").unwrap());
}

#[test]
fn streaming_propagates_exit_code() {
    let mut sh = Shell::with_memory_vfs();
    let mut out = VecSink::new();
    let mut err = VecSink::new();

    // false-y: cat of a missing file returns 1
    let code = sh.feed_streaming("cat /nope.txt", &mut out, &mut err);
    assert_eq!(code, 1);
    assert!(!err.as_slice().is_empty(), "expected stderr to be written");
}

#[test]
fn streaming_separates_stdout_and_stderr() {
    let mut sh = Shell::with_memory_vfs();
    let mut out = VecSink::new();
    let mut err = VecSink::new();

    // `unknown-cmd` triggers the command-not-found stderr path
    let code = sh.feed_streaming("definitely-not-a-real-command", &mut out, &mut err);
    assert_eq!(code, 127);
    assert!(out.as_slice().is_empty(), "stdout should be empty");
    let err_text = String::from_utf8_lossy(err.as_slice());
    assert!(
        err_text.contains("command not found"),
        "got stderr: {:?}",
        err_text
    );
}

#[test]
fn osc_side_effects_appear_inline_with_output() {
    // `./whoami.sh` triggers an OSC redirect side effect. With streaming, that
    // OSC sequence must flush through the sink at the moment the builtin
    // emitted it — not coalesced at the end.
    let mut sh = Shell::with_memory_vfs();
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let _ = sh.feed_streaming("./whoami.sh", &mut out, &mut err);

    let out_text = String::from_utf8_lossy(out.as_slice());
    assert!(
        out_text.contains("\x1b]9999;"),
        "expected OSC 9999 side-effect sequence in stdout, got: {:?}",
        out_text
    );
}
