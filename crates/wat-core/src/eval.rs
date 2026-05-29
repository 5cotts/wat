use crate::ast::{Command, List, Pipeline, Redirect, Separator};
use crate::builtins::resolve::resolve_path;
use crate::builtins::run_builtin;
use crate::context::Context;
use crate::expand::expand_word;
use crate::io::ShellIo;

/// Evaluate a parsed command list. Returns `(exit_code, combined_output)`.
/// Both stdout and stderr are interleaved in the output string (stderr is mixed in unless redirected).
pub fn eval(list: &List, ctx: &mut Context) -> (i32, String) {
    let mut combined = Vec::<u8>::new();
    let mut last_code = 0i32;

    let mut iter = list.0.iter().peekable();
    while let Some((pipeline, sep)) = iter.next() {
        let (code, stdout, stderr) = eval_pipeline(pipeline, ctx, &[]);
        combined.extend_from_slice(&stdout);
        combined.extend_from_slice(&stderr);
        ctx.env.last_exit_code = code;
        last_code = code;

        match sep {
            Separator::And => {
                if code != 0 && iter.peek().is_some() {
                    iter.next();
                }
            }
            Separator::Or => {
                if code == 0 && iter.peek().is_some() {
                    iter.next();
                }
            }
            Separator::Semi | Separator::End => {}
        }
    }

    (last_code, String::from_utf8_lossy(&combined).into_owned())
}

/// Run a pipeline, chaining stdout of each command to stdin of the next.
/// Returns `(exit_code, stdout_bytes, stderr_bytes)`.
fn eval_pipeline(pipeline: &Pipeline, ctx: &mut Context, initial_stdin: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let cmds = &pipeline.0;
    let mut stdin_data: Vec<u8> = initial_stdin.to_vec();
    let mut all_stderr: Vec<u8> = Vec::new();
    let mut last_code = 0;

    for (idx, cmd) in cmds.iter().enumerate() {
        let is_last = idx + 1 == cmds.len();
        let (code, stdout, stderr) = run_command(cmd, ctx, &stdin_data);
        all_stderr.extend_from_slice(&stderr);
        last_code = code;
        ctx.env.last_exit_code = code;

        if is_last {
            return (last_code, stdout, all_stderr);
        }
        stdin_data = stdout;
    }

    (0, Vec::new(), all_stderr)
}

/// Run a single command, handling redirects. Returns `(exit_code, stdout, stderr)`.
fn run_command(cmd: &Command, ctx: &mut Context, stdin_data: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let name = expand_word(&cmd.name, &ctx.env);
    let args: Vec<String> = cmd.args.iter().map(|a| expand_word(a, &ctx.env)).collect();

    // Determine effective stdin (may be overridden by `< file`)
    let stdin_bytes: Vec<u8> = cmd
        .redirects
        .iter()
        .find_map(|r| {
            if let Redirect::In(path) = r {
                let full = resolve_path(path, &ctx.env.cwd);
                ctx.vfs.read(&full).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| stdin_data.to_vec());

    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();

    let code = {
        let mut io = ShellIo { stdin: &stdin_bytes, stdout: &mut stdout, stderr: &mut stderr };
        match run_builtin(&name, &args, ctx, &mut io) {
            Some(c) => c,
            None => {
                io.write_err(&format!("wat: command not found: {}\n", name));
                127
            }
        }
    };

    // Apply stdout redirects (`>` and `>>`)
    for redirect in &cmd.redirects {
        match redirect {
            Redirect::Out(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let _ = ctx.vfs.write(&full, &stdout);
                stdout.clear();
            }
            Redirect::Append(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let mut existing = ctx.vfs.read(&full).unwrap_or_default();
                existing.extend_from_slice(&stdout);
                let _ = ctx.vfs.write(&full, &existing);
                stdout.clear();
            }
            Redirect::Err(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let _ = ctx.vfs.write(&full, &stderr);
                stderr.clear();
            }
            Redirect::In(_) => {} // already handled above
        }
    }

    (code, stdout, stderr)
}
