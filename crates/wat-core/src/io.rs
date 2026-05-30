/// A sink for shell output. Builtins and external commands write byte chunks
/// here as they are produced. `feed_streaming` invokes the sink in order, so
/// callers can observe output incrementally rather than waiting for the whole
/// command to finish.
pub trait OutputSink {
    fn write(&mut self, chunk: &[u8]);
}

/// Sink that appends all writes to an in-memory buffer. Used by the buffered
/// `Shell::feed` wrapper and as the per-segment buffer inside pipelines.
pub struct VecSink {
    buf: Vec<u8>,
}

impl VecSink {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

impl Default for VecSink {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputSink for VecSink {
    fn write(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }
}

/// Sink that writes directly to the process's stdout. Native only.
#[cfg(feature = "native-fs")]
pub struct StdoutSink;

#[cfg(feature = "native-fs")]
impl OutputSink for StdoutSink {
    fn write(&mut self, chunk: &[u8]) {
        use std::io::Write as _;
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(chunk);
        let _ = out.flush();
    }
}

/// Sink that writes directly to the process's stderr. Native only.
#[cfg(feature = "native-fs")]
pub struct StderrSink;

#[cfg(feature = "native-fs")]
impl OutputSink for StderrSink {
    fn write(&mut self, chunk: &[u8]) {
        use std::io::Write as _;
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(chunk);
        let _ = err.flush();
    }
}

/// Shell I/O context passed to every builtin. `stdin` is a byte slice the
/// builtin reads from; `stdout` and `stderr` are sinks the builtin writes into.
pub struct ShellIo<'a> {
    pub stdin: &'a [u8],
    pub stdout: &'a mut dyn OutputSink,
    pub stderr: &'a mut dyn OutputSink,
}

impl<'a> ShellIo<'a> {
    pub fn write_out(&mut self, s: &str) {
        self.stdout.write(s.as_bytes());
    }

    pub fn write_out_bytes(&mut self, b: &[u8]) {
        self.stdout.write(b);
    }

    pub fn write_err(&mut self, s: &str) {
        self.stderr.write(s.as_bytes());
    }

    pub fn write_err_bytes(&mut self, b: &[u8]) {
        self.stderr.write(b);
    }

    pub fn stdin_str(&self) -> &str {
        std::str::from_utf8(self.stdin).unwrap_or("")
    }

    /// Emit an OSC 9999 side-effect escape sequence into stdout. The browser's
    /// shell-bridge.ts parses and dispatches these. They flush through the
    /// sink in order, so anything written after them appears after them on the
    /// terminal.
    pub fn emit_side_effect(&mut self, effect: &SideEffect) {
        let json = effect.to_json();
        self.write_out(&format!("\x1b]9999;{}\x07", json));
    }
}

/// Side effects that the WASM shell asks the browser to perform via OSC 9999.
pub enum SideEffect {
    Redirect { url: String, delay_ms: Option<u32> },
    KonamiCelebrate,
    PersistVfs { snapshot: String },
}

impl SideEffect {
    fn to_json(&self) -> String {
        match self {
            SideEffect::Redirect { url, delay_ms } => match delay_ms {
                Some(d) => format!(r#"{{"type":"redirect","url":"{}","delay_ms":{}}}"#, url, d),
                None => format!(r#"{{"type":"redirect","url":"{}"}}"#, url),
            },
            SideEffect::KonamiCelebrate => r#"{"type":"konami_celebrate"}"#.to_string(),
            SideEffect::PersistVfs { snapshot } => {
                format!(r#"{{"type":"persist_vfs","snapshot":"{}"}}"#, snapshot)
            }
        }
    }
}
