//! `jobs`, `fg`, `bg` builtins for interactive job control (Tier 3).
//!
//! These builtins run in-process but CANNOT enter raw mode or drive a PTY
//! — that machinery lives in `wat-cli`. Instead, `fg`/`bg` communicate with
//! the REPL loop via `ctx.pending_fg` / `ctx.pending_bg`, which are checked
//! after each builtin returns.

use crate::context::Context;
use crate::io::ShellIo;
use crate::jobs::JobState;

pub fn jobs_builtin(ctx: &Context, io: &mut ShellIo) -> i32 {
    let table = ctx.jobs.lock().expect("job table");
    let most_recent = table.most_recent();
    let jobs: Vec<_> = table.iter().collect();
    if jobs.is_empty() {
        return 0;
    }
    let second_recent = {
        let ids: Vec<u32> = jobs.iter().map(|j| j.id).collect();
        let len = ids.len();
        if len >= 2 {
            Some(ids[len - 2])
        } else {
            None
        }
    };
    for job in &jobs {
        let mark = if Some(job.id) == most_recent {
            '+'
        } else if Some(job.id) == second_recent {
            '-'
        } else {
            ' '
        };
        let state_str = match job.state {
            JobState::Running => "Running",
            JobState::Stopped => "Stopped",
            JobState::Done(_) => "Done",
        };
        io.write_out(&format!(
            "[{}]{} {:<20}{}\n",
            job.id, mark, state_str, job.cmd
        ));
    }
    0
}

pub fn fg_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let id = resolve_job_id(args, ctx, io, "fg");
    match id {
        Some(n) => {
            ctx.pending_fg = Some(n);
            0
        }
        None => 1,
    }
}

pub fn bg_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let id = resolve_job_id(args, ctx, io, "bg");
    match id {
        Some(n) => {
            ctx.pending_bg = Some(n);
            0
        }
        None => 1,
    }
}

/// `kill [-SIG] %N | <pid>` — send a signal (default TERM) to a job's process
/// group or to a raw pid. Job death is observed asynchronously by the REPL's
/// SIGCHLD handler, which prints the Done/Exit notification at the next prompt.
pub fn kill_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if args.is_empty() {
        io.write_err("wat: kill: usage: kill [-SIG] %job | pid\n");
        return 2;
    }

    // Optional leading signal spec: -9, -KILL, -SIGKILL, -TERM, ...
    let (signum, rest) = if let Some(first) = args.first().filter(|a| a.starts_with('-')) {
        match parse_signal(&first[1..]) {
            Some(n) => (n, &args[1..]),
            None => {
                io.write_err(&format!("wat: kill: {}: invalid signal\n", first));
                return 1;
            }
        }
    } else {
        (15, args) // SIGTERM
    };

    let target = match rest.first() {
        Some(t) => t,
        None => {
            io.write_err("wat: kill: usage: kill [-SIG] %job | pid\n");
            return 2;
        }
    };

    if let Some(job_spec) = target.strip_prefix('%') {
        // Job target → signal the whole process group.
        let id = match job_spec.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                io.write_err(&format!("wat: kill: {}: invalid job id\n", target));
                return 1;
            }
        };
        let table = ctx.jobs.lock().expect("job table");
        let pgid = match table.get(id) {
            Some(job) => job.pgid,
            None => {
                io.write_err(&format!("wat: kill: %{}: no such job\n", id));
                return 1;
            }
        };
        send_signal(-pgid, signum, io, &format!("%{}", id))
    } else {
        // Raw pid target.
        match target.parse::<i32>() {
            Ok(pid) => send_signal(pid, signum, io, target),
            Err(_) => {
                io.write_err(&format!(
                    "wat: kill: {}: arguments must be job ids or pids\n",
                    target
                ));
                1
            }
        }
    }
}

/// Map a signal spec (`9`, `KILL`, `SIGKILL`) to its number.
fn parse_signal(spec: &str) -> Option<i32> {
    if let Ok(n) = spec.parse::<i32>() {
        return Some(n);
    }
    let name = spec
        .strip_prefix("SIG")
        .unwrap_or(spec)
        .to_ascii_uppercase();
    Some(match name.as_str() {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "KILL" => 9,
        "TERM" => 15,
        "STOP" => 19,
        "CONT" => 18,
        "USR1" => 10,
        "USR2" => 12,
        _ => return None,
    })
}

#[cfg(unix)]
fn send_signal(target: i32, signum: i32, io: &mut ShellIo, label: &str) -> i32 {
    extern "C" {
        #[link_name = "kill"]
        fn libc_kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: kill(2) is a plain syscall; we tolerate failure (ESRCH, EPERM).
    let rc = unsafe { libc_kill(target, signum) };
    if rc == 0 {
        0
    } else {
        let err = std::io::Error::last_os_error();
        io.write_err(&format!("wat: kill: {}: {}\n", label, err));
        1
    }
}

#[cfg(not(unix))]
fn send_signal(_target: i32, _signum: i32, io: &mut ShellIo, _label: &str) -> i32 {
    io.write_err("wat: kill: not supported on this platform\n");
    1
}

/// Parse an optional `%N` argument; fall back to the most-recent job.
fn resolve_job_id(args: &[String], ctx: &Context, io: &mut ShellIo, builtin: &str) -> Option<u32> {
    let table = ctx.jobs.lock().expect("job table");
    let id = if let Some(arg) = args.first() {
        let s = arg.trim_start_matches('%');
        match s.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                io.write_err(&format!("wat: {}: invalid job id: {}\n", builtin, arg));
                return None;
            }
        }
    } else {
        match table.most_recent() {
            Some(n) => n,
            None => {
                io.write_err(&format!("wat: {}: no current job\n", builtin));
                return None;
            }
        }
    };
    if table.get(id).is_none() {
        io.write_err(&format!("wat: {}: %{}: no such job\n", builtin, id));
        return None;
    }
    Some(id)
}
