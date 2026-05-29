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
}
