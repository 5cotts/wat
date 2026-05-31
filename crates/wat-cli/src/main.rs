use is_terminal::IsTerminal;
use std::io::{self, BufRead, Read, Write};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use wat_core::io::{StderrSink, StdoutSink};
use wat_core::process::NativeProcessHost;
use wat_core::pty::{NativePtyHost, PtyChild, PtyDims};
use wat_core::Shell;

fn main() {
    let mut shell = Shell::new()
        .with_process_host(Box::new(NativeProcessHost))
        .with_pty_host(Box::new(NativePtyHost));

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

    let cancel = shell.cancel_flag();
    signal_hook::flag::register(signal_hook::consts::SIGINT, cancel.clone())
        .expect("install SIGINT handler");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let stdin_is_tty = io::stdin().is_terminal();

    print!("{}", shell.prompt());
    stdout.lock().flush().unwrap();

    // Read one line at a time, releasing the stdin lock before processing so
    // the stdin→master thread inside drive_pty_job can acquire it.
    loop {
        let line = {
            let mut lock = stdin.lock();
            let mut buf = String::new();
            match lock.read_line(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            buf.trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string()
        };
        cancel.store(false, Ordering::Relaxed);

        // Print Done notifications for background jobs that finished.
        drain_done_notifications(&shell);

        // Handle pending fg set by the `fg` builtin (Phase C).
        if let Some(id) = shell.ctx.pending_fg.take() {
            let exit = resume_fg(&mut shell, id);
            shell.set_last_exit_code(exit);
            if shell.exit_requested {
                std::process::exit(shell.last_exit_code());
            }
            print!("{}", shell.prompt());
            stdout.lock().flush().unwrap();
            continue;
        }

        // Handle pending bg set by the `bg` builtin (Phase C).
        if let Some(id) = shell.ctx.pending_bg.take() {
            bg_job(&shell, id);
        }

        if stdin_is_tty && shell.pty_eligible(&line) {
            let exit = run_in_pty(&mut shell, &line);
            shell.set_last_exit_code(exit);
        } else {
            let mut out = StdoutSink;
            let mut err = StderrSink;
            shell.feed_streaming(&line, &mut out, &mut err);
            if cancel.swap(false, Ordering::Relaxed) {
                println!("^C");
            }
        }
        if shell.exit_requested {
            std::process::exit(shell.last_exit_code());
        }

        // Check pending_fg/bg set by builtins (fg/bg) during THIS command.
        if let Some(id) = shell.ctx.pending_fg.take() {
            let exit = resume_fg(&mut shell, id);
            shell.set_last_exit_code(exit);
            if shell.exit_requested {
                std::process::exit(shell.last_exit_code());
            }
        } else if let Some(id) = shell.ctx.pending_bg.take() {
            bg_job(&shell, id);
        }

        print!("{}", shell.prompt());
        stdout.lock().flush().unwrap();
    }
}

/// Print Done/Exit notifications for background jobs that finished.
fn drain_done_notifications(shell: &Shell) {
    use wat_core::jobs::JobState;
    let jobs_arc = shell.jobs();
    let mut table = jobs_arc.lock().expect("job table");
    let done_ids: Vec<u32> = table
        .iter()
        .filter(|j| matches!(j.state, JobState::Done(_)))
        .map(|j| j.id)
        .collect();
    for id in done_ids {
        if let Some(job) = table.remove(id) {
            match job.state {
                JobState::Done(0) => eprintln!("[{}]+ Done\t\t{}", job.id, job.cmd),
                JobState::Done(code) => eprintln!("[{}]+ Exit {}\t\t{}", job.id, code, job.cmd),
                _ => {}
            }
        }
    }
}

/// Send SIGCONT to a stopped job without re-entering the drive loop.
fn bg_job(shell: &Shell, id: u32) {
    use wat_core::jobs::JobState;
    let jobs_arc = shell.jobs();
    let mut table = jobs_arc.lock().expect("job table");
    if let Some(job) = table.get_mut(id) {
        unsafe {
            extern "C" {
                fn kill(pid: i32, sig: i32) -> i32;
            }
            kill(-(job.pgid), 18); // SIGCONT
        }
        eprintln!("[{}] continued\t\t{}", job.id, job.cmd);
        job.state = JobState::Running;
    } else {
        eprintln!("wat: bg: %{}: no such job", id);
    }
}

/// Resume a stopped job in the foreground.
fn resume_fg(shell: &mut Shell, id: u32) -> i32 {
    let job = {
        let jobs_arc = shell.jobs();
        let mut table = jobs_arc.lock().expect("job table");
        match table.remove(id) {
            Some(j) => j,
            None => {
                eprintln!("wat: fg: %{}: no such job", id);
                return 1;
            }
        }
    };

    let pgid = job.pgid;
    let cmd = job.cmd.clone();

    unsafe {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        kill(-pgid, 18); // SIGCONT
    }
    eprintln!("[{}] continued\t\t{}", id, cmd);

    let mut pty = match job.pty {
        Some(p) => p,
        None => {
            eprintln!("wat: fg: job %{} has no pty handles", id);
            return 1;
        }
    };

    let reader = match pty.child.clone_reader() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("wat: fg: {}", e);
            return 1;
        }
    };

    let writer = match pty.writer.take() {
        Some(w) => w,
        None => match pty.child.master_writer() {
            Some(w) => w,
            None => {
                eprintln!("wat: fg: no writer available for job %{}", id);
                return 1;
            }
        },
    };

    let child_arc = Arc::new(Mutex::new(pty.child));
    let cancel_pipe = CancelPipe::new();
    let (exit, stopped, child_arc) = drive_pty_job(reader, writer, child_arc, &cmd, &cancel_pipe);

    if stopped {
        register_stopped_job(shell, child_arc, pgid, cmd);
    }
    exit
}

/// Register a stopped child in the job table and print the notification.
fn register_stopped_job(
    shell: &mut Shell,
    child_arc: Arc<Mutex<Box<dyn PtyChild>>>,
    fallback_pgid: i32,
    cmd: String,
) {
    use wat_core::jobs::{JobPty, JobState};

    let pid = child_arc
        .lock()
        .expect("child")
        .pid()
        .unwrap_or(fallback_pgid);

    match Arc::try_unwrap(child_arc) {
        Ok(mutex) => {
            let child_box = mutex.into_inner().expect("mutex");
            let pty = JobPty {
                child: child_box,
                reader: None,
                writer: None,
            };
            let jobs_arc = shell.jobs();
            let mut table = jobs_arc.lock().expect("job table");
            let jid = table.add(cmd.clone(), pid, pty);
            table.get_mut(jid).unwrap().state = JobState::Stopped;
            eprintln!("\n[{}]+ Stopped\t\t{}", jid, cmd);
        }
        Err(arc) => {
            // Arc still has references (shouldn't happen if SIGWINCH joined).
            drop(arc);
            eprintln!("\n[1]+ Stopped\t\t{}", cmd);
        }
    }
}

/// Self-pipe cancellation token for the stdin→master thread. The thread polls
/// on fd 0 (stdin) and the read end of the pipe; writing to the write end
/// wakes the thread so it can exit cleanly.
struct CancelPipe {
    #[cfg(unix)]
    read_fd: i32,
    #[cfg(unix)]
    write_fd: i32,
}

impl CancelPipe {
    fn new() -> Self {
        #[cfg(unix)]
        {
            let mut fds = [0i32; 2];
            let rc = unsafe { libc_pipe(fds.as_mut_ptr()) };
            if rc != 0 {
                panic!("CancelPipe::new: pipe() failed");
            }
            CancelPipe {
                read_fd: fds[0],
                write_fd: fds[1],
            }
        }
        #[cfg(not(unix))]
        CancelPipe {}
    }

    /// Signal the thread to stop.
    fn cancel(&self) {
        #[cfg(unix)]
        {
            let _ = unsafe { libc_write(self.write_fd, [0u8].as_ptr() as *const _, 1) };
        }
    }

    #[cfg(unix)]
    fn read_fd(&self) -> i32 {
        self.read_fd
    }
}

impl Drop for CancelPipe {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            unsafe {
                libc_close(self.read_fd);
                libc_close(self.write_fd);
            }
        }
    }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "pipe"]
    fn libc_pipe(fds: *mut i32) -> i32;
    #[link_name = "write"]
    fn libc_write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
    #[link_name = "close"]
    fn libc_close(fd: i32) -> i32;
    #[link_name = "read"]
    fn libc_read_raw(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
    #[link_name = "poll"]
    fn libc_poll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
}

/// Minimal poll(2) wrapper struct.
#[cfg(unix)]
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[cfg(unix)]
const POLLIN: i16 = 1;

/// RAII guard around `crossterm::terminal::enable_raw_mode`.
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

enum PtyMsg {
    Out(Vec<u8>),
    Done,
}

/// Spawn a PTY child and drive the SIGCHLD-aware read loop.
fn run_in_pty(shell: &mut Shell, input: &str) -> i32 {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let dims = PtyDims { rows, cols };
    let mut child = match shell.spawn_pty(input, dims) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wat: {}", e);
            return 127;
        }
    };
    let reader = child.master_reader().expect("master reader");
    let writer = child.master_writer().expect("master writer");
    let child_arc = Arc::new(Mutex::new(child));
    let cancel_pipe = CancelPipe::new();

    let (exit, stopped, child_arc) = drive_pty_job(reader, writer, child_arc, input, &cancel_pipe);

    if stopped {
        let pid = child_arc.lock().expect("child").pid().unwrap_or(0);
        register_stopped_job(shell, child_arc, pid, input.trim().to_string());
    }

    exit
}

/// Inner drive loop. Returns `(exit_code, was_stopped, child_arc)`.
fn drive_pty_job(
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    child: Arc<Mutex<Box<dyn PtyChild>>>,
    _cmd: &str,
    cancel: &CancelPipe,
) -> (i32, bool, Arc<Mutex<Box<dyn PtyChild>>>) {
    use std::sync::mpsc;
    use std::time::Duration;
    use wat_core::process::ChildState;

    let _guard = match RawModeGuard::enter() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("wat: enter raw mode: {}", e);
            return (1, false, child);
        }
    };

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
                if let Ok(mut c) = child_for_winch.lock() {
                    let _ = c.resize(PtyDims { rows, cols });
                }
            }
        });
        (Some(handle), Some(join))
    };
    #[cfg(not(unix))]
    let (winch_handle, winch_thread): (Option<()>, Option<std::thread::JoinHandle<()>>) =
        (None, None);

    // stdin → master via raw fd poll, with cancellation. Bypasses io::stdin()
    // lock so the REPL can re-acquire stdin after the drive loop exits.
    #[cfg(unix)]
    let cancel_read_fd = cancel.read_fd();
    let stdin_thread = std::thread::spawn(move || {
        let mut writer = writer;
        let mut buf = [0u8; 1024];
        loop {
            #[cfg(unix)]
            {
                // poll(stdin_fd=0, cancel_fd) with 200ms timeout.
                let mut pfds = [
                    PollFd {
                        fd: 0,
                        events: POLLIN,
                        revents: 0,
                    },
                    PollFd {
                        fd: cancel_read_fd,
                        events: POLLIN,
                        revents: 0,
                    },
                ];
                let rc = unsafe { libc_poll(pfds.as_mut_ptr(), 2, 200) };
                if rc < 0 {
                    break; // poll error
                }
                if pfds[1].revents & POLLIN != 0 {
                    break; // cancelled
                }
                if pfds[0].revents & POLLIN == 0 {
                    continue; // timeout, no data
                }
                // Data on stdin.
                let n = unsafe { libc_read_raw(0, buf.as_mut_ptr() as *mut _, buf.len()) };
                if n <= 0 {
                    break;
                }
                if writer.write_all(&buf[..n as usize]).is_err() {
                    break;
                }
                let _ = writer.flush();
            }
            #[cfg(not(unix))]
            {
                let stdin = io::stdin();
                let mut stdin = stdin.lock();
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
        }
    });

    // master → channel.
    let (tx, rx) = mpsc::channel::<PtyMsg>();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(PtyMsg::Done);
                    break;
                }
                Ok(n) => {
                    if tx.send(PtyMsg::Out(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(PtyMsg::Done);
                    break;
                }
            }
        }
    });

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut stopped = false;
    let mut exit_code = 0i32;

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(PtyMsg::Out(b)) => {
                let _ = stdout.write_all(&b);
                let _ = stdout.flush();
            }
            Ok(PtyMsg::Done) => {
                let code = child.lock().expect("child mutex").wait().unwrap_or(1);
                exit_code = code;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let state = child
                    .lock()
                    .expect("child mutex")
                    .try_wait()
                    .unwrap_or(ChildState::Running);
                match state {
                    ChildState::Running => {}
                    ChildState::Stopped { signum } => {
                        stopped = true;
                        exit_code = 128 + signum;
                        break;
                    }
                    ChildState::Exited(code) => {
                        exit_code = code;
                        break;
                    }
                    ChildState::Signaled(signum) => {
                        exit_code = 128 + signum;
                        break;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(stdout);

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

    // Signal the stdin→master thread to stop and wait for it to exit.
    // This ensures no raw fd 0 reads race with the REPL's next stdin.lock().
    cancel.cancel();
    let _ = stdin_thread.join();

    (exit_code, stopped, child)
}
