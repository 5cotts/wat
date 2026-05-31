use crate::env::Env;
use crate::history::History;
use crate::process::{NoopProcessHost, ProcessHost};
use crate::vfs::{MemoryVfs, Vfs};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Loop control requested by the `break`/`continue` builtins, consumed by the
/// enclosing loop evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopCtl {
    Break,
    Continue,
}

/// Combines the shell environment, VFS, history, and host capabilities
/// (process spawning) — passed to eval and builtins.
pub struct Context {
    pub env: Env,
    pub vfs: Box<dyn Vfs>,
    pub history: History,
    pub process_host: Box<dyn ProcessHost>,
    /// SIGINT cancellation flag. The native CLI installs a `signal-hook`
    /// handler that flips this to `true`; the pipeline executor polls it
    /// while draining child output and forwards `Signal::Interrupt` to the
    /// foreground child. WASM never flips this, so the flag is harmless
    /// dead-weight there.
    pub cancel: Arc<AtomicBool>,
    /// Optional PTY host for interactive foreground commands. The native
    /// CLI installs `NativePtyHost` here; everything else (WASM, tests
    /// that don't need a PTY, non-interactive callers) leaves it `None`
    /// and falls back to the piped `ProcessHost` path.
    #[cfg(feature = "native-pty")]
    pub pty_host: Option<Box<dyn crate::pty::PtyHost>>,
    /// Job table for stopped/background PTY children. Shared with wat-cli
    /// via `Shell::jobs()`.
    #[cfg(feature = "native-pty")]
    pub jobs: Arc<std::sync::Mutex<crate::jobs::JobTable>>,
    /// Requested foreground job id, set by the `fg` builtin and consumed
    /// by the REPL loop in wat-cli.
    #[cfg(feature = "native-pty")]
    pub pending_fg: Option<u32>,
    /// Requested background-resume job id, set by the `bg` builtin.
    #[cfg(feature = "native-pty")]
    pub pending_bg: Option<u32>,
    /// Current command-substitution nesting depth. Guards against runaway
    /// recursion (`$($($(...)))`) blowing the stack. Not feature-gated:
    /// command substitution works on every target, including WASM.
    pub subst_depth: u32,
    /// How many loops (`for`/`while`/`until`) are currently executing. The
    /// `break`/`continue` builtins are only meaningful when this is > 0.
    pub loop_depth: u32,
    /// Pending loop control set by `break`/`continue`, consumed by the
    /// enclosing loop evaluator.
    pub loop_ctl: Option<LoopCtl>,
    /// Set by the `exit` builtin to the requested status. `eval_streaming`
    /// stops the current list when this is `Some`, so `exit` terminates a
    /// script mid-way; `Shell::feed*` surfaces it as `exit_requested`.
    pub exit_status: Option<i32>,
    /// Defined shell functions (name → body AST). `Rc` so a call can clone the
    /// handle cheaply and release the table borrow before evaluating the body.
    pub functions: HashMap<String, Rc<crate::ast::Command>>,
    /// How many function calls are currently executing; `return` is only
    /// meaningful when > 0.
    pub fn_depth: u32,
    /// Pending `return` status, consumed by the function-call evaluator.
    pub returning: Option<i32>,
    /// `set -e` (errexit): abort the current list when a simple command exits
    /// non-zero, except in condition contexts (see `errexit_suppressed`).
    pub opt_errexit: bool,
    /// `set -u` (nounset): expanding an unset variable is an error.
    pub opt_nounset: bool,
    /// `set -x` (xtrace): print each simple command to stderr before running it.
    pub opt_xtrace: bool,
    /// Temporarily disables `errexit` while evaluating the condition of an
    /// `if`/`while`/`until` (those failures must not abort the shell).
    pub errexit_suppressed: bool,
}

impl Context {
    pub fn new() -> Self {
        #[cfg(feature = "native-fs")]
        let vfs: Box<dyn Vfs> = Box::new(crate::vfs::NativeVfs::new());
        #[cfg(not(feature = "native-fs"))]
        let vfs: Box<dyn Vfs> = Box::new(MemoryVfs::new_seeded());

        Context {
            env: Env::new(),
            vfs,
            history: History::new(100),
            process_host: Box::new(NoopProcessHost),
            cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "native-pty")]
            pty_host: None,
            #[cfg(feature = "native-pty")]
            jobs: Arc::new(std::sync::Mutex::new(crate::jobs::JobTable::new())),
            #[cfg(feature = "native-pty")]
            pending_fg: None,
            #[cfg(feature = "native-pty")]
            pending_bg: None,
            subst_depth: 0,
            loop_depth: 0,
            loop_ctl: None,
            exit_status: None,
            functions: HashMap::new(),
            fn_depth: 0,
            returning: None,
            opt_errexit: false,
            opt_nounset: false,
            opt_xtrace: false,
            errexit_suppressed: false,
        }
    }

    /// For testing — use a fresh MemoryVfs regardless of features.
    pub fn with_memory_vfs() -> Self {
        Context {
            env: Env::new(),
            vfs: Box::new(MemoryVfs::new_seeded()),
            history: History::new(100),
            process_host: Box::new(NoopProcessHost),
            cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "native-pty")]
            pty_host: None,
            #[cfg(feature = "native-pty")]
            jobs: Arc::new(std::sync::Mutex::new(crate::jobs::JobTable::new())),
            #[cfg(feature = "native-pty")]
            pending_fg: None,
            #[cfg(feature = "native-pty")]
            pending_bg: None,
            subst_depth: 0,
            loop_depth: 0,
            loop_ctl: None,
            exit_status: None,
            functions: HashMap::new(),
            fn_depth: 0,
            returning: None,
            opt_errexit: false,
            opt_nounset: false,
            opt_xtrace: false,
            errexit_suppressed: false,
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
