use std::io::{self, BufRead, Write};
use std::sync::atomic::Ordering;
use wat_core::io::{StderrSink, StdoutSink};
use wat_core::process::NativeProcessHost;
use wat_core::Shell;

fn main() {
    let mut shell = Shell::new().with_process_host(Box::new(NativeProcessHost));

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

    print!("{}", shell.prompt());
    stdout.lock().flush().unwrap();

    for line in stdin.lock().lines() {
        let line = line.expect("failed to read line");
        // Reset the cancel flag at the start of each command so a Ctrl-C
        // that arrived during the previous prompt-read doesn't immediately
        // cancel the next command.
        cancel.store(false, Ordering::Relaxed);
        // Stream stdout/stderr directly to the terminal as the command
        // produces them — long-running externals (e.g. `cargo build`) show
        // progress live instead of dumping everything at the end.
        let mut out = StdoutSink;
        let mut err = StderrSink;
        shell.feed_streaming(&line, &mut out, &mut err);
        if cancel.swap(false, Ordering::Relaxed) {
            // Mimic bash/zsh: print the visible Ctrl-C marker so the user
            // can see why the command stopped. The line buffer is already
            // empty (we just consumed `line`) so there's nothing extra to
            // clear.
            println!("^C");
        }
        if shell.exit_requested {
            std::process::exit(shell.last_exit_code());
        }
        print!("{}", shell.prompt());
        stdout.lock().flush().unwrap();
    }
}
