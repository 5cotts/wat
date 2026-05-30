use crate::complete::complete;
use crate::context::Context;
use crate::eval::{eval, eval_streaming};
use crate::io::{OutputSink, VecSink};
use crate::parser::parse;
use crate::process::ProcessHost;

pub struct Shell {
    pub ctx: Context,
    pub exit_requested: bool,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            ctx: Context::new(),
            exit_requested: false,
        }
    }

    pub fn with_memory_vfs() -> Self {
        Self {
            ctx: Context::with_memory_vfs(),
            exit_requested: false,
        }
    }

    /// Replace the process host. Used by the native CLI to swap in a
    /// `NativeProcessHost`; the default is `NoopProcessHost` so the WASM
    /// target keeps the "command not found" behavior for unknown commands.
    pub fn with_process_host(mut self, host: Box<dyn ProcessHost>) -> Self {
        self.ctx.process_host = host;
        self
    }

    pub fn prompt(&self) -> String {
        format!("5cotts@zo {} % ", self.ctx.env.prompt_cwd())
    }

    /// Buffered API: evaluates `input` and returns the combined output as a
    /// `String`. Stderr is interleaved into the returned string when not
    /// redirected. Kept for the WASM bridge and existing callers.
    pub fn feed(&mut self, input: &str) -> String {
        let input = input.trim();
        if input.is_empty() {
            return String::new();
        }

        if let Some(code) = self.handle_exit(input) {
            self.ctx.env.last_exit_code = code;
            self.exit_requested = true;
            return String::new();
        }

        self.ctx.history.push(input);

        match parse(input) {
            Ok(list) => {
                let (_, output) = eval(&list, &mut self.ctx);
                output
            }
            Err(e) => format!("wat: {}\n", e),
        }
    }

    /// Streaming API: evaluates `input` and forwards stdout/stderr to the
    /// supplied sinks as they are produced. Returns the exit code of the last
    /// pipeline. Stderr is forwarded to `err`; callers that want POSIX-style
    /// interleaving can pass the same sink for both.
    pub fn feed_streaming(
        &mut self,
        input: &str,
        out: &mut dyn OutputSink,
        err: &mut dyn OutputSink,
    ) -> i32 {
        let input = input.trim();
        if input.is_empty() {
            return self.ctx.env.last_exit_code;
        }

        if let Some(code) = self.handle_exit(input) {
            self.ctx.env.last_exit_code = code;
            self.exit_requested = true;
            return code;
        }

        self.ctx.history.push(input);

        match parse(input) {
            Ok(list) => {
                let code = eval_streaming(&list, &mut self.ctx, out, err);
                self.ctx.env.last_exit_code = code;
                code
            }
            Err(e) => {
                let msg = format!("wat: {}\n", e);
                err.write(msg.as_bytes());
                self.ctx.env.last_exit_code = 2;
                2
            }
        }
    }

    /// Convenience for callers (mainly the native CLI) that want one combined
    /// stream. Backed internally by a `VecSink` so it adds no real cost beyond
    /// what `feed` already does.
    pub fn feed_into(&mut self, input: &str, sink: &mut dyn OutputSink) -> i32 {
        let mut err = VecSink::new();
        let code = self.feed_streaming(input, sink, &mut err);
        sink.write(err.as_slice());
        code
    }

    fn handle_exit(&self, input: &str) -> Option<i32> {
        if input == "exit" {
            return Some(0);
        }
        if let Some(rest) = input.strip_prefix("exit ") {
            return Some(rest.trim().parse::<i32>().unwrap_or(0));
        }
        None
    }

    pub fn complete(&self, input: &str, cursor: usize) -> Vec<String> {
        complete(input, cursor, &self.ctx.env.cwd, self.ctx.vfs.as_ref())
    }

    pub fn history_at(&self, index: usize) -> Option<String> {
        self.ctx.history.at(index).map(|s| s.to_string())
    }

    pub fn last_exit_code(&self) -> i32 {
        self.ctx.env.last_exit_code
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}
