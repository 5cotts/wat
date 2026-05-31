//! Subprocess execution abstraction.
//!
//! The shell evaluator only ever talks to a `ProcessHost`. The native CLI
//! wires this up to `std::process::Command` behind the `native-proc` feature.
//! The WASM target uses `NoopProcessHost`, which always reports "unsupported"
//! and keeps every byte of `std::process` out of the bundle.

use std::io;
use std::path::PathBuf;

/// A signal that can be delivered to a running child process. Only Interrupt
/// is plumbed today (Phase E); the enum is here so the trait surface is stable.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
}

/// Non-blocking child state returned by `PtyChild::try_wait`.
#[cfg(feature = "native-pty")]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChildState {
    Running,
    Stopped {
        signum: i32,
    },
    Exited(i32),
    /// Killed by a signal; callers encode exit code as 128 + signum.
    Signaled(i32),
}

/// Errors that can occur while resolving or spawning a command.
#[derive(Debug)]
pub enum ProcessError {
    /// The host does not support spawning processes (e.g. WASM).
    Unsupported,
    /// The host supports spawning, but the underlying syscall failed.
    Io(io::Error),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::Unsupported => write!(f, "process execution not supported on this host"),
            ProcessError::Io(e) => write!(f, "spawn error: {}", e),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<io::Error> for ProcessError {
    fn from(e: io::Error) -> Self {
        ProcessError::Io(e)
    }
}

/// What the parent shell wants to feed into the child's stdin.
pub enum ChildStdin {
    /// Pipe the given bytes in, then close stdin.
    Bytes(Vec<u8>),
    /// Inherit the parent's stdin.
    Inherit,
    /// Close stdin immediately (equivalent to redirecting from /dev/null).
    Null,
    /// Stream from a pre-existing reader (typically the previous external
    /// segment's `take_stdout()`). The host spawns a writer thread that
    /// copies the reader into the child's stdin pipe — a "software pipe".
    /// When the downstream child closes its stdin (e.g. `head -1` exits),
    /// the writer drops the reader and the upstream child sees SIGPIPE on
    /// its next write.
    Pipe(Box<dyn std::io::Read + Send>),
}

/// What to run, where, and with what env.
pub struct ProcessSpec {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// A running child process. Stdout and stderr are exposed as detachable
/// `Read + Send` handles so the caller can drain both concurrently from
/// reader threads — `read_stdout`/`read_stderr` would deadlock against each
/// other if either pipe filled.
pub trait ChildProcess {
    /// Take ownership of the child's stdout pipe. Returns `None` if the child
    /// was spawned without a piped stdout, or if it has already been taken.
    fn take_stdout(&mut self) -> Option<Box<dyn io::Read + Send>>;
    /// Take ownership of the child's stderr pipe. Same caveats as
    /// `take_stdout`.
    fn take_stderr(&mut self) -> Option<Box<dyn io::Read + Send>>;
    fn wait(&mut self) -> io::Result<i32>;
    fn signal(&mut self, sig: Signal) -> io::Result<()>;
}

/// Host abstraction for finding and launching external programs.
pub trait ProcessHost {
    /// Resolve `name` against the host's PATH (or equivalent). Returns the
    /// absolute path to the executable if found, otherwise `None`.
    fn lookup(&self, name: &str) -> Option<PathBuf>;

    /// Launch a process. The caller is responsible for reading from
    /// stdout/stderr and waiting for the child.
    fn spawn(
        &self,
        spec: ProcessSpec,
        stdin: ChildStdin,
    ) -> Result<Box<dyn ChildProcess>, ProcessError>;
}

/// A `ProcessHost` that refuses every spawn. Used in WASM and as the default
/// when no other host is configured.
pub struct NoopProcessHost;

impl ProcessHost for NoopProcessHost {
    fn lookup(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    fn spawn(
        &self,
        _spec: ProcessSpec,
        _stdin: ChildStdin,
    ) -> Result<Box<dyn ChildProcess>, ProcessError> {
        Err(ProcessError::Unsupported)
    }
}

// ---------------------------------------------------------------------------
// Native implementation, behind the `native-proc` feature.
// ---------------------------------------------------------------------------

#[cfg(feature = "native-proc")]
mod native {
    use super::*;
    use std::process::{Child, Command, Stdio};

    /// Resolve `name` against the host's PATH. If `name` contains a path
    /// separator we treat it as an explicit path and check it directly.
    pub fn lookup_in_path(name: &str) -> Option<PathBuf> {
        if name.contains(std::path::MAIN_SEPARATOR) {
            let p = PathBuf::from(name);
            return is_executable(&p).then_some(p);
        }
        let path_var = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn is_executable(p: &std::path::Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::metadata(p)
                .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false)
        }
    }

    pub struct NativeProcessHost;

    impl ProcessHost for NativeProcessHost {
        fn lookup(&self, name: &str) -> Option<PathBuf> {
            lookup_in_path(name)
        }

        fn spawn(
            &self,
            spec: ProcessSpec,
            stdin: ChildStdin,
        ) -> Result<Box<dyn ChildProcess>, ProcessError> {
            let mut cmd = Command::new(&spec.argv[0]);
            cmd.args(&spec.argv[1..]);
            cmd.current_dir(&spec.cwd);
            // Inherit the parent's environment, then layer the shell's vars
            // on top so e.g. PATH/HOME/PWD from the shell take precedence.
            for (k, v) in spec.env.iter() {
                cmd.env(k, v);
            }
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.stdin(match &stdin {
                ChildStdin::Bytes(_) | ChildStdin::Pipe(_) => Stdio::piped(),
                ChildStdin::Inherit => Stdio::inherit(),
                ChildStdin::Null => Stdio::null(),
            });

            let mut child = cmd.spawn().map_err(ProcessError::Io)?;
            match stdin {
                ChildStdin::Bytes(bytes) => {
                    if let Some(mut stdin_handle) = child.stdin.take() {
                        use std::io::Write as _;
                        let _ = stdin_handle.write_all(&bytes);
                        // dropping closes the pipe so the child sees EOF
                    }
                }
                ChildStdin::Pipe(mut reader) => {
                    if let Some(mut stdin_handle) = child.stdin.take() {
                        std::thread::spawn(move || {
                            // copy() returns on EOF of the reader or on
                            // BrokenPipe from the writer (downstream child
                            // closed its stdin). Either way we just exit;
                            // dropping `reader` closes the upstream side,
                            // which propagates SIGPIPE back to the producer.
                            let _ = std::io::copy(&mut reader, &mut stdin_handle);
                        });
                    }
                }
                ChildStdin::Inherit | ChildStdin::Null => {}
            }
            Ok(Box::new(NativeChild { inner: child }))
        }
    }

    pub struct NativeChild {
        inner: Child,
    }

    impl ChildProcess for NativeChild {
        fn take_stdout(&mut self) -> Option<Box<dyn io::Read + Send>> {
            self.inner
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn io::Read + Send>)
        }

        fn take_stderr(&mut self) -> Option<Box<dyn io::Read + Send>> {
            self.inner
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn io::Read + Send>)
        }

        fn wait(&mut self) -> io::Result<i32> {
            let status = self.inner.wait()?;
            // POSIX: 128 + signum when killed by signal, otherwise the exit code.
            Ok(status.code().unwrap_or_else(|| {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt as _;
                    status.signal().map(|s| 128 + s).unwrap_or(1)
                }
                #[cfg(not(unix))]
                {
                    1
                }
            }))
        }

        fn signal(&mut self, sig: Signal) -> io::Result<()> {
            #[cfg(unix)]
            {
                let pid = self.inner.id() as i32;
                let num = match sig {
                    Signal::Interrupt => 2, // SIGINT
                };
                let rc = unsafe { libc_kill(pid, num) };
                if rc == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            #[cfg(not(unix))]
            {
                let _ = sig;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "signal not supported on this platform",
                ))
            }
        }
    }

    #[cfg(unix)]
    extern "C" {
        #[link_name = "kill"]
        fn libc_kill(pid: i32, sig: i32) -> i32;
    }
}

#[cfg(feature = "native-proc")]
pub use native::{lookup_in_path, NativeChild, NativeProcessHost};
