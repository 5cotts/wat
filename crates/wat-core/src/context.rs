use crate::env::Env;
use crate::history::History;
use crate::process::{NoopProcessHost, ProcessHost};
use crate::vfs::{MemoryVfs, Vfs};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
