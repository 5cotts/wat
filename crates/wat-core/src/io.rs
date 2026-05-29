/// Shell I/O context passed to every builtin.
/// `stdin` is a byte slice the builtin reads from.
/// `stdout` and `stderr` are byte buffers the builtin writes into.
pub struct ShellIo<'a> {
    pub stdin: &'a [u8],
    pub stdout: &'a mut Vec<u8>,
    pub stderr: &'a mut Vec<u8>,
}

impl<'a> ShellIo<'a> {
    pub fn write_out(&mut self, s: &str) {
        self.stdout.extend_from_slice(s.as_bytes());
    }

    pub fn write_err(&mut self, s: &str) {
        self.stderr.extend_from_slice(s.as_bytes());
    }

    pub fn stdin_str(&self) -> &str {
        std::str::from_utf8(self.stdin).unwrap_or("")
    }

    /// Emit an OSC 9999 side-effect escape sequence into stdout.
    /// The browser's shell-bridge.ts parses and dispatches these.
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
