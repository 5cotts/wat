//! Pseudo-terminal (PTY) execution for full-screen / raw-mode programs.
//!
//! Tier 1 spawns externals with `Stdio::piped()` for stdin/stdout/stderr, so
//! children see pipes — `isatty(1)` is false, no colors, no full-screen TUIs.
//! For interactive foreground commands the native CLI wants the opposite:
//! a real PTY pair so `vim`, `less`, `htop`, etc. think they're talking to a
//! terminal.
//!
//! This module lives behind the `native-pty` feature so the WASM build never
//! pulls in `portable-pty` and the bundle size doesn't grow.

use crate::process::{ChildState, ProcessError, ProcessSpec, Signal};
use std::io;

/// Terminal dimensions, in cells. Matches `winsize`'s rows/cols.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PtyDims {
    pub rows: u16,
    pub cols: u16,
}

/// A child running inside a PTY. The parent reads child output via
/// `master_reader` and feeds keystrokes via `master_writer`; both may only
/// be taken once (the same `Option`-take pattern as `ChildProcess`).
///
/// `resize` is how SIGWINCH gets forwarded — the native CLI's signal
/// handler calls this whenever the user resizes their terminal.
pub trait PtyChild: Send {
    fn master_reader(&mut self) -> Option<Box<dyn io::Read + Send>>;
    fn master_writer(&mut self) -> Option<Box<dyn io::Write + Send>>;
    fn resize(&mut self, dims: PtyDims) -> io::Result<()>;
    /// Non-blocking poll of child state. Returns `ChildState::Running` if the
    /// child has not changed state since the last call.
    fn try_wait(&mut self) -> io::Result<ChildState>;
    /// Blocking wait. Loops on `try_wait` until a terminal state is reached.
    fn wait(&mut self) -> io::Result<i32>;
    fn signal(&mut self, sig: Signal) -> io::Result<()>;
    /// Return the child's PID, if known.
    fn pid(&self) -> Option<i32>;
}

/// Host abstraction for launching commands inside a PTY. Separate from
/// `ProcessHost` so the WASM build can stay cleanly free of `portable-pty`
/// — there is intentionally no `NoopPtyHost`.
pub trait PtyHost: Send + Sync {
    fn spawn_pty(
        &self,
        spec: ProcessSpec,
        dims: PtyDims,
    ) -> Result<Box<dyn PtyChild>, ProcessError>;
}

// ---------------------------------------------------------------------------
// Native implementation, behind `native-pty`. Only compiled when wat-cli
// pulls it in; wat-wasm never sees this code.
// ---------------------------------------------------------------------------

#[cfg(feature = "native-pty")]
mod native {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
    use std::sync::Mutex;
    use std::time::Duration;

    pub struct NativePtyHost;

    impl PtyHost for NativePtyHost {
        fn spawn_pty(
            &self,
            spec: ProcessSpec,
            dims: PtyDims,
        ) -> Result<Box<dyn PtyChild>, ProcessError> {
            let pty_system = native_pty_system();
            let pair = pty_system
                .openpty(PtySize {
                    rows: dims.rows,
                    cols: dims.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| ProcessError::Io(io::Error::other(e.to_string())))?;

            let mut builder = CommandBuilder::new(&spec.argv[0]);
            if spec.argv.len() > 1 {
                builder.args(&spec.argv[1..]);
            }
            // CommandBuilder starts with an empty env (no parent inheritance),
            // which matches what we want — the shell's env already includes
            // the merged set we care about (PATH, HOME, PWD, etc.).
            for (k, v) in spec.env.iter() {
                builder.env(k, v);
            }
            builder.cwd(&spec.cwd);

            let child = pair
                .slave
                .spawn_command(builder)
                .map_err(|e| ProcessError::Io(io::Error::other(e.to_string())))?;

            // Capture the PID before dropping slave. process_id() is valid
            // immediately after spawn.
            let pid = child.process_id().map(|p| p as i32);

            // Drop the slave handle on the parent side. The child still owns
            // its FDs to the slave, but the parent no longer needs them —
            // and holding on would keep the PTY open past child exit, which
            // would prevent `master_reader` from ever seeing EOF.
            drop(pair.slave);

            // `portable-pty` exposes the reader/writer as two separate
            // factories (`try_clone_reader`, `take_writer`) plus the master
            // for `resize`. We need to hold the master alive for the lifetime
            // of the child so `resize` keeps working, hence the `Mutex` (the
            // trait methods are `&mut self` but `try_clone_reader` /
            // `take_writer` aren't on the same trait object once we Box it).
            let reader = pair
                .master
                .try_clone_reader()
                .map_err(|e| ProcessError::Io(io::Error::other(e.to_string())))?;
            let writer = pair
                .master
                .take_writer()
                .map_err(|e| ProcessError::Io(io::Error::other(e.to_string())))?;

            Ok(Box::new(NativePtyChild {
                master: Mutex::new(pair.master),
                reader: Some(reader),
                writer: Some(writer),
                // Keep the portable-pty child alive so its cleanup runs on drop,
                // but we no longer call its wait()/try_wait() — we use waitpid directly.
                _child: child,
                pid,
            }))
        }
    }

    pub struct NativePtyChild {
        master: Mutex<Box<dyn MasterPty + Send>>,
        pub reader: Option<Box<dyn io::Read + Send>>,
        pub writer: Option<Box<dyn io::Write + Send>>,
        /// Held alive for Drop (closes slave FDs), but never waited on.
        _child: Box<dyn portable_pty::Child + Send + Sync>,
        /// PID for direct waitpid calls. Set to None once the child is reaped.
        pub pid: Option<i32>,
    }

    impl PtyChild for NativePtyChild {
        fn master_reader(&mut self) -> Option<Box<dyn io::Read + Send>> {
            self.reader.take()
        }

        fn master_writer(&mut self) -> Option<Box<dyn io::Write + Send>> {
            self.writer.take()
        }

        fn resize(&mut self, dims: PtyDims) -> io::Result<()> {
            let m = self.master.lock().expect("master mutex poisoned");
            m.resize(PtySize {
                rows: dims.rows,
                cols: dims.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| io::Error::other(e.to_string()))
        }

        fn pid(&self) -> Option<i32> {
            self.pid
        }

        #[cfg(unix)]
        fn try_wait(&mut self) -> io::Result<ChildState> {
            let pid = match self.pid {
                Some(p) => p,
                None => return Ok(ChildState::Exited(0)),
            };
            let mut status: i32 = 0;
            // WNOHANG | WUNTRACED | WCONTINUED
            let rc = unsafe { libc_waitpid(pid, &mut status, 1 | 2 | 8) };
            if rc < 0 {
                let err = io::Error::last_os_error();
                // ECHILD means the child was already reaped
                if err.raw_os_error() == Some(10) {
                    self.pid = None;
                    return Ok(ChildState::Exited(0));
                }
                return Err(err);
            }
            if rc == 0 {
                return Ok(ChildState::Running);
            }
            // Decode the raw status word
            if wif_stopped(status) {
                return Ok(ChildState::Stopped {
                    signum: wstop_sig(status),
                });
            }
            if wif_exited(status) {
                self.pid = None;
                return Ok(ChildState::Exited(wex_status(status)));
            }
            if wif_signaled(status) {
                self.pid = None;
                return Ok(ChildState::Signaled(wterm_sig(status)));
            }
            // WCONTINUED — child resumed; treat as Running
            Ok(ChildState::Running)
        }

        #[cfg(not(unix))]
        fn try_wait(&mut self) -> io::Result<ChildState> {
            Ok(ChildState::Running)
        }

        fn wait(&mut self) -> io::Result<i32> {
            loop {
                match self.try_wait()? {
                    ChildState::Running => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    ChildState::Stopped { signum } => return Ok(128 + signum),
                    ChildState::Exited(code) => return Ok(code),
                    ChildState::Signaled(signum) => return Ok(128 + signum),
                }
            }
        }

        fn signal(&mut self, sig: Signal) -> io::Result<()> {
            #[cfg(unix)]
            {
                let pid = match self.pid {
                    Some(p) => p,
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "pty child already reaped",
                        ))
                    }
                };
                let num = match sig {
                    Signal::Interrupt => 2, // SIGINT
                };
                // SAFETY: kill is async-signal-safe; pid is the child's PID,
                // and we tolerate ESRCH if the child raced to exit first.
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

    // Status word decoding (POSIX, Linux/macOS compatible)
    fn wif_exited(status: i32) -> bool {
        (status & 0x7f) == 0
    }
    fn wex_status(status: i32) -> i32 {
        (status >> 8) & 0xff
    }
    fn wif_signaled(status: i32) -> bool {
        let lo = (status & 0x7f) as i8;
        lo != 0 && lo != 127
    }
    fn wterm_sig(status: i32) -> i32 {
        status & 0x7f
    }
    fn wif_stopped(status: i32) -> bool {
        (status & 0xff) == 0x7f
    }
    fn wstop_sig(status: i32) -> i32 {
        (status >> 8) & 0xff
    }

    #[cfg(unix)]
    extern "C" {
        #[link_name = "kill"]
        fn libc_kill(pid: i32, sig: i32) -> i32;

        #[link_name = "waitpid"]
        fn libc_waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }
}

#[cfg(feature = "native-pty")]
pub use native::{NativePtyChild, NativePtyHost};
