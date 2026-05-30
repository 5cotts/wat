use crate::complete::complete;
use crate::context::Context;
use crate::eval::{eval, eval_streaming};
use crate::io::{OutputSink, VecSink};
use crate::parser::parse;
use crate::process::ProcessHost;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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

    /// Hand back the shared cancel flag so the caller (e.g. the native CLI)
    /// can install a SIGINT handler that flips it. While the pipeline is
    /// running, the executor polls this flag and forwards
    /// `Signal::Interrupt` to the foreground child. Callers should reset it
    /// to `false` after handling a cancellation.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.ctx.cancel.clone()
    }

    /// Install a PTY host so the native CLI can spawn interactive
    /// foreground commands (`vim`, `less`, ...) inside a real pseudo-
    /// terminal. WASM never calls this; the WASM build doesn't even
    /// compile the `pty` module.
    #[cfg(feature = "native-pty")]
    pub fn with_pty_host(mut self, host: Box<dyn crate::pty::PtyHost>) -> Self {
        self.ctx.pty_host = Some(host);
        self
    }

    /// Parse `input`, resolve the leading command, and spawn it inside a
    /// PTY. Returns the running child to the caller, who is responsible
    /// for shuttling bytes between the user's terminal and the master
    /// pipe and for calling `wait()`. Used only by the native CLI's
    /// interactive path; tests and scripts go through `feed_streaming`.
    ///
    /// Returns `ProcessError::Unsupported` if no `PtyHost` is installed,
    /// if the input doesn't parse to a single-command pipeline with no
    /// redirects, or if the command name isn't found on PATH.
    #[cfg(feature = "native-pty")]
    pub fn spawn_pty(
        &mut self,
        input: &str,
        dims: crate::pty::PtyDims,
    ) -> Result<Box<dyn crate::pty::PtyChild>, crate::process::ProcessError> {
        use crate::expand::expand_word;
        use crate::glob::glob_expand;
        use crate::process::{ProcessError, ProcessSpec};

        let input = input.trim();
        if input.is_empty() {
            return Err(ProcessError::Unsupported);
        }
        self.ctx.history.push(input);

        let list = parse(input).map_err(|_| ProcessError::Unsupported)?;
        if list.0.len() != 1 {
            return Err(ProcessError::Unsupported);
        }
        let (pipeline, _) = &list.0[0];
        if pipeline.0.len() != 1 {
            return Err(ProcessError::Unsupported);
        }
        let cmd = &pipeline.0[0];
        if !cmd.redirects.is_empty() {
            return Err(ProcessError::Unsupported);
        }

        let name = expand_word(&cmd.name, &self.ctx.env);
        let args: Vec<String> = cmd
            .args
            .iter()
            .flat_map(|a| {
                let expanded = expand_word(a, &self.ctx.env);
                glob_expand(&expanded, self.ctx.vfs.as_ref(), &self.ctx.env.cwd)
            })
            .collect();

        let path = self
            .ctx
            .process_host
            .lookup(&name)
            .ok_or(ProcessError::Unsupported)?;

        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(path.to_string_lossy().into_owned());
        argv.extend(args);
        let env: Vec<(String, String)> = self
            .ctx
            .env
            .vars()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let spec = ProcessSpec {
            argv,
            env,
            cwd: std::path::PathBuf::from(&self.ctx.env.cwd),
        };

        let host = self
            .ctx
            .pty_host
            .as_ref()
            .ok_or(ProcessError::Unsupported)?;
        host.spawn_pty(spec, dims)
    }

    /// After a PTY child exits, store its exit code in `$?` so subsequent
    /// commands can reference it.
    #[cfg(feature = "native-pty")]
    pub fn set_last_exit_code(&mut self, code: i32) {
        self.ctx.env.last_exit_code = code;
    }

    /// Returns true if `input` should be routed through the PTY path
    /// instead of the buffered piped path. Used by `wat-cli` (combined
    /// with a TTY check on its own stdin).
    ///
    /// True iff:
    /// 1. `input` parses cleanly.
    /// 2. The parsed list is a single pipeline of a single command (no
    ///    `;`, no `&&`/`||`, no `|`).
    /// 3. That command has no redirects (no `<`, `>`, `>>`, `2>`).
    /// 4. The command name does not resolve to a wat builtin.
    /// 5. The command name resolves on PATH via `process_host.lookup`.
    #[cfg(feature = "native-pty")]
    pub fn pty_eligible(&self, input: &str) -> bool {
        use crate::expand::expand_word;

        let trimmed = input.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Ok(list) = parse(trimmed) else {
            return false;
        };
        if list.0.len() != 1 {
            return false;
        }
        let (pipeline, _sep) = &list.0[0];
        if pipeline.0.len() != 1 {
            return false;
        }
        let cmd = &pipeline.0[0];
        if !cmd.redirects.is_empty() {
            return false;
        }
        let name = expand_word(&cmd.name, &self.ctx.env);
        if crate::builtins::is_builtin(&name) {
            return false;
        }
        self.ctx.process_host.lookup(&name).is_some()
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
