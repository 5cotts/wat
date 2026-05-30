use is_terminal::IsTerminal;
use std::io::{self, BufRead, Read, Write};
use std::sync::atomic::Ordering;
use wat_core::io::{StderrSink, StdoutSink};
use wat_core::process::NativeProcessHost;
use wat_core::pty::{NativePtyHost, PtyDims};
use wat_core::Shell;

fn main() {
    let mut shell = Shell::new()
        .with_process_host(Box::new(NativeProcessHost))
        .with_pty_host(Box::new(NativePtyHost));

    // The default shell env points at the in-memory VFS layout (/home/5cotts).
    // In native mode we want to land on the host's actual cwd so spawned
    // children inherit a directory that really exists. HOME / PWD are derived
    // from the host environment for the same reason.
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.to_string_lossy().into_owned();
        shell.ctx.env.cwd = cwd_str.clone();
        shell.ctx.env.set("PWD", cwd_str);
    }
    if let Ok(home) = std::env::var("HOME") {
        shell.ctx.env.set("HOME", home);
    }
    if let Ok(path) = std::env::var("PATH") {
        shell.ctx.env.set("PATH", path);
    }

    // Wire up Ctrl-C. `signal_hook::flag::register` installs an
    // async-signal-safe handler that flips the shared atomic flag instead
    // of the default SIGINT behavior (terminate). The pipeline executor
    // polls this flag while draining child output and forwards
    // `Signal::Interrupt` to the foreground child. The same SIGINT also
    // reaches the child directly via the terminal foreground process group,
    // so for well-behaved children (e.g. `sleep`) the kernel-delivered
    // signal usually wins; our explicit `child.signal()` is the backstop
    // for anything that ignores its terminal SIGINT.
    let cancel = shell.cancel_flag();
    signal_hook::flag::register(signal_hook::consts::SIGINT, cancel.clone())
        .expect("install SIGINT handler");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let stdin_is_tty = io::stdin().is_terminal();

    print!("{}", shell.prompt());
    stdout.lock().flush().unwrap();

    for line in stdin.lock().lines() {
        let line = line.expect("failed to read line");
        // Reset the cancel flag at the start of each command so a Ctrl-C
        // that arrived during the previous prompt-read doesn't immediately
        // cancel the next command.
        cancel.store(false, Ordering::Relaxed);

        // PTY path for interactive foreground commands when our stdin is
        // a real TTY. The routing rule (single-command pipeline, no
        // redirects, not a builtin, resolves on PATH) lives in
        // `Shell::pty_eligible` so it can stay in sync with the parser
        // and the builtin set.
        if stdin_is_tty && shell.pty_eligible(&line) {
            let exit = run_in_pty(&mut shell, &line);
            shell.set_last_exit_code(exit);
        } else {
            // Stream stdout/stderr directly to the terminal as the command
            // produces them — long-running externals (e.g. `cargo build`)
            // show progress live instead of dumping everything at the end.
            let mut out = StdoutSink;
            let mut err = StderrSink;
            shell.feed_streaming(&line, &mut out, &mut err);
            if cancel.swap(false, Ordering::Relaxed) {
                // Mimic bash/zsh: print the visible Ctrl-C marker so the
                // user can see why the command stopped. The line buffer is
                // already empty (we just consumed `line`) so there's
                // nothing extra to clear.
                println!("^C");
            }
        }
        if shell.exit_requested {
            std::process::exit(shell.last_exit_code());
        }
        print!("{}", shell.prompt());
        stdout.lock().flush().unwrap();
    }
}

/// RAII guard around `crossterm::terminal::enable_raw_mode`. Drop restores
/// cooked mode on every exit path — normal return, panic, early `?`. The
/// CLI MUST go through this guard; manual `enable` / `disable` pairs are
/// too easy to leak on a panic.
struct RawModeGuard;
impl RawModeGuard {
    fn enter() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Drive a PTY-spawned child. Allocates a PTY at the current terminal
/// size, enters raw mode, copies bytes both directions until the child
/// exits, then restores cooked mode (via the guard's `Drop`) before
/// returning the exit code.
///
/// Returns 127 if the command can't be resolved or spawned. The stdin
/// reader thread is intentionally detached — it may be parked in a
/// blocking `read` on stdin even after the child exits, and joining
/// would hang the REPL until the user pressed a key. Leaking it per
/// command is acceptable for Tier 2; Tier 3 will revisit.
///
/// SIGWINCH (terminal resize) is forwarded via a background thread that
/// reads from a `signal_hook::iterator::Signals`. When the user resizes
/// their terminal, the thread queries the new size from crossterm and
/// calls `child.resize(...)` so apps like `less` re-paint at the new
/// dimensions immediately.
fn run_in_pty(shell: &mut Shell, input: &str) -> i32 {
    use std::sync::{Arc, Mutex};

    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let dims = PtyDims { rows, cols };
    let mut child = match shell.spawn_pty(input, dims) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wat: {}", e);
            return 127;
        }
    };
    let mut reader = child.master_reader().expect("master reader");
    let mut writer = child.master_writer().expect("master writer");
    // From here on `child` only needs `resize()` (from the SIGWINCH thread)
    // and `wait()` (from this thread, at the end). Wrap in Arc<Mutex<>> so
    // both can share it.
    let child = Arc::new(Mutex::new(child));

    let _guard = match RawModeGuard::enter() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("wat: enter raw mode: {}", e);
            return 1;
        }
    };

    // SIGWINCH forwarding. signal-hook installs an async-signal-safe
    // handler internally; `Signals::forever()` blocks on a self-pipe in
    // this background thread and yields events as they arrive. On each
    // event we query the new terminal size and call `child.resize`. When
    // we close the handle at the bottom of this function, `forever()`
    // returns `None` and the thread exits.
    #[cfg(unix)]
    let (winch_handle, winch_thread) = {
        use signal_hook::iterator::Signals;
        let mut signals =
            Signals::new([signal_hook::consts::SIGWINCH]).expect("install SIGWINCH handler");
        let handle = signals.handle();
        let child_for_winch = child.clone();
        let join = std::thread::spawn(move || {
            for _signal in signals.forever() {
                let Ok((cols, rows)) = crossterm::terminal::size() else {
                    continue;
                };
                if let Ok(mut child) = child_for_winch.lock() {
                    let _ = child.resize(PtyDims { rows, cols });
                }
            }
        });
        (Some(handle), Some(join))
    };
    #[cfg(not(unix))]
    let (winch_handle, winch_thread): (Option<()>, Option<std::thread::JoinHandle<()>>) =
        (None, None);

    // stdin → master, in a detached thread.
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        let mut buf = [0u8; 1024];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if writer.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
                Err(_) => break,
            }
        }
    });

    // master → stdout on this thread; we exit promptly when the master
    // EOFs (which happens when the child closes its slave FDs on exit).
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = stdout.write_all(&buf[..n]);
                let _ = stdout.flush();
            }
            Err(_) => break,
        }
    }

    let code = child.lock().expect("child mutex").wait().unwrap_or(1);

    // Tear down the SIGWINCH handler before returning so a resize at the
    // prompt doesn't try to call `resize` on a dead child.
    #[cfg(unix)]
    {
        if let Some(h) = winch_handle {
            h.close();
        }
        if let Some(t) = winch_thread {
            let _ = t.join();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (winch_handle, winch_thread);
    }

    code
}
