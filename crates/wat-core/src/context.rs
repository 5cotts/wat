use crate::env::Env;
use crate::vfs::{MemoryVfs, Vfs};

/// Combines the shell environment with the VFS — passed to eval and builtins.
pub struct Context {
    pub env: Env,
    pub vfs: Box<dyn Vfs>,
}

impl Context {
    pub fn new() -> Self {
        #[cfg(feature = "native-fs")]
        let vfs: Box<dyn Vfs> = Box::new(crate::vfs::NativeVfs::new());
        #[cfg(not(feature = "native-fs"))]
        let vfs: Box<dyn Vfs> = Box::new(MemoryVfs::new_seeded());

        Context { env: Env::new(), vfs }
    }

    /// For testing — use a fresh MemoryVfs regardless of features.
    pub fn with_memory_vfs() -> Self {
        Context { env: Env::new(), vfs: Box::new(MemoryVfs::new_seeded()) }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
