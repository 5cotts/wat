use crate::env::Env;
use crate::history::History;
use crate::process::{NoopProcessHost, ProcessHost};
use crate::vfs::{MemoryVfs, Vfs};

/// Combines the shell environment, VFS, history, and host capabilities
/// (process spawning) — passed to eval and builtins.
pub struct Context {
    pub env: Env,
    pub vfs: Box<dyn Vfs>,
    pub history: History,
    pub process_host: Box<dyn ProcessHost>,
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
        }
    }

    /// For testing — use a fresh MemoryVfs regardless of features.
    pub fn with_memory_vfs() -> Self {
        Context {
            env: Env::new(),
            vfs: Box::new(MemoryVfs::new_seeded()),
            history: History::new(100),
            process_host: Box::new(NoopProcessHost),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
