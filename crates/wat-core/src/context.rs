use crate::env::Env;
use crate::history::History;
use crate::vfs::{MemoryVfs, Vfs};

/// Combines the shell environment, VFS, and history — passed to eval and builtins.
pub struct Context {
    pub env: Env,
    pub vfs: Box<dyn Vfs>,
    pub history: History,
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
        }
    }

    /// For testing — use a fresh MemoryVfs regardless of features.
    pub fn with_memory_vfs() -> Self {
        Context {
            env: Env::new(),
            vfs: Box::new(MemoryVfs::new_seeded()),
            history: History::new(100),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
