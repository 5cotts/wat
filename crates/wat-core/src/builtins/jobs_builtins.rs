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
