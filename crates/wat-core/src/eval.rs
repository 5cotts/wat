use crate::ast::{Command, List, Pipeline, Redirect, Separator};
use crate::builtins::resolve::resolve_path;
use crate::builtins::run_builtin;
use crate::context::Context;
use crate::expand::expand_word;
use crate::glob::glob_expand;
use crate::io::{OutputSink, ShellIo, VecSink};
#[cfg(feature = "native-proc")]
use crate::process::{ChildStdin, ProcessError, ProcessSpec};

/// Evaluate a parsed command list, streaming output into the supplied sinks
/// as it is produced. Returns the exit code of the last pipeline.
pub fn eval_streaming(
    list: &List,
    ctx: &mut Context,
    out: &mut dyn OutputSink,
    err: &mut dyn OutputSink,
) -> i32 {
    let mut last_code = 0i32;

    let mut iter = list.0.iter().peekable();
    while let Some((pipeline, sep)) = iter.next() {
        let code = eval_pipeline(pipeline, ctx, &[], out, err);
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

    last_code
}

/// Buffered convenience wrapper around `eval_streaming` that returns
/// `(exit_code, combined_output_string)`. Stderr is interleaved into the
/// returned string, matching the pre-streaming behavior.
pub fn eval(list: &List, ctx: &mut Context) -> (i32, String) {
    let mut out = VecSink::new();
    let mut err = VecSink::new();
    let code = eval_streaming(list, ctx, &mut out, &mut err);
    let mut combined = out.into_inner();
    combined.extend_from_slice(err.as_slice());
    (code, String::from_utf8_lossy(&combined).into_owned())
}

/// Run a pipeline, chaining stdout of each command to stdin of the next.
/// Inner segments buffer their stdout into a `VecSink` so it can become the
/// next segment's stdin. The final segment streams stdout into `final_out`.
/// Stderr from every segment is forwarded to `final_err` as it is produced.
fn eval_pipeline(
    pipeline: &Pipeline,
    ctx: &mut Context,
    initial_stdin: &[u8],
    final_out: &mut dyn OutputSink,
    final_err: &mut dyn OutputSink,
) -> i32 {
    let cmds = &pipeline.0;
    let mut stdin_data: Vec<u8> = initial_stdin.to_vec();
    let mut last_code = 0i32;

    for (idx, cmd) in cmds.iter().enumerate() {
        let is_last = idx + 1 == cmds.len();
        if is_last {
            let code = run_command(cmd, ctx, &stdin_data, final_out, final_err);
            ctx.env.last_exit_code = code;
            last_code = code;
        } else {
            let mut buffered_out = VecSink::new();
            let code = run_command(cmd, ctx, &stdin_data, &mut buffered_out, final_err);
            ctx.env.last_exit_code = code;
            last_code = code;
            stdin_data = buffered_out.into_inner();
        }
    }

    last_code
}

/// Run a single command, handling redirects and writing output to the supplied
/// sinks. If the command has any output redirects, output is buffered locally
/// so it can be routed to the VFS instead of the outer sinks.
fn run_command(
    cmd: &Command,
    ctx: &mut Context,
    stdin_data: &[u8],
    out_sink: &mut dyn OutputSink,
    err_sink: &mut dyn OutputSink,
) -> i32 {
    let name = expand_word(&cmd.name, &ctx.env);
    let args: Vec<String> = cmd
        .args
        .iter()
        .flat_map(|a| {
            let expanded = expand_word(a, &ctx.env);
            glob_expand(&expanded, ctx.vfs.as_ref(), &ctx.env.cwd)
        })
        .collect();

    let name = if (name == "bash" || name == "sh")
        && args.first().map(|s| s.as_str()) == Some("whoami.sh")
    {
        "bash whoami.sh".to_string()
    } else {
        name
    };

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

    let has_out_redirect = cmd
        .redirects
        .iter()
        .any(|r| matches!(r, Redirect::Out(_) | Redirect::Append(_)));
    let has_err_redirect = cmd.redirects.iter().any(|r| matches!(r, Redirect::Err(_)));

    let mut local_out = VecSink::new();
    let mut local_err = VecSink::new();

    let code = {
        let stdout_target: &mut dyn OutputSink = if has_out_redirect {
            &mut local_out
        } else {
            out_sink
        };
        let stderr_target: &mut dyn OutputSink = if has_err_redirect {
            &mut local_err
        } else {
            err_sink
        };
        run_one(
            &name,
            &args,
            ctx,
            &stdin_bytes,
            stdout_target,
            stderr_target,
        )
    };

    for redirect in &cmd.redirects {
        match redirect {
            Redirect::Out(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let _ = ctx.vfs.write(&full, local_out.as_slice());
            }
            Redirect::Append(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let mut existing = ctx.vfs.read(&full).unwrap_or_default();
                existing.extend_from_slice(local_out.as_slice());
                let _ = ctx.vfs.write(&full, &existing);
            }
            Redirect::Err(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let _ = ctx.vfs.write(&full, local_err.as_slice());
            }
            Redirect::In(_) => {}
        }
    }

    code
}

/// Try builtin first; if it doesn't match, ask the ProcessHost; if that
/// doesn't resolve either, emit "command not found".
fn run_one(
    name: &str,
    args: &[String],
    ctx: &mut Context,
    stdin_bytes: &[u8],
    out_sink: &mut dyn OutputSink,
    err_sink: &mut dyn OutputSink,
) -> i32 {
    {
        let mut io = ShellIo {
            stdin: stdin_bytes,
            stdout: out_sink,
            stderr: err_sink,
        };
        if let Some(code) = run_builtin(name, args, ctx, &mut io) {
            return code;
        }
    }

    // External execution path: only compiled in when `native-proc` is enabled.
    // In the WASM build this whole block — including the threading machinery
    // in `stream_child` — disappears, keeping the bundle small. The
    // `NoopProcessHost` would refuse to spawn anyway, so the user-visible
    // behavior is identical.
    #[cfg(feature = "native-proc")]
    {
        if let Some(path) = ctx.process_host.lookup(name) {
            let mut argv = Vec::with_capacity(args.len() + 1);
            argv.push(path.to_string_lossy().into_owned());
            argv.extend(args.iter().cloned());
            let env: Vec<(String, String)> = ctx
                .env
                .vars()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let spec = ProcessSpec {
                argv,
                env,
                cwd: std::path::PathBuf::from(&ctx.env.cwd),
            };
            let stdin = if stdin_bytes.is_empty() {
                ChildStdin::Null
            } else {
                ChildStdin::Bytes(stdin_bytes.to_vec())
            };
            return match ctx.process_host.spawn(spec, stdin) {
                Ok(mut child) => stream_child(&mut *child, out_sink, err_sink),
                Err(ProcessError::Unsupported) => {
                    let msg = format!("wat: command not found: {}\n", name);
                    err_sink.write(msg.as_bytes());
                    127
                }
                Err(ProcessError::Io(e)) => {
                    let msg = format!("wat: {}: {}\n", name, e);
                    err_sink.write(msg.as_bytes());
                    126
                }
            };
        }
    }
    #[cfg(not(feature = "native-proc"))]
    {
        let _ = (ctx, stdin_bytes, out_sink);
    }

    let msg = format!("wat: command not found: {}\n", name);
    err_sink.write(msg.as_bytes());
    127
}

/// Drain a running child's stdout/stderr into the supplied sinks until both
/// pipes are at EOF, then wait for the child and return its exit code. Uses
/// two reader threads + a tagged channel so the main thread can write the
/// non-Send sinks while the pipes drain concurrently — no deadlock if the
/// child writes a lot to stderr before flushing stdout.
///
/// Native-only: pulls in `std::thread` and `mpsc`, both of which inflate the
/// WASM bundle when included (and `std::thread::spawn` panics under wasm32
/// at runtime anyway).
#[cfg(feature = "native-proc")]
fn stream_child(
    child: &mut dyn crate::process::ChildProcess,
    out_sink: &mut dyn OutputSink,
    err_sink: &mut dyn OutputSink,
) -> i32 {
    use std::sync::mpsc;
    use std::thread;

    enum Msg {
        Out(Vec<u8>),
        Err(Vec<u8>),
        OutDone,
        ErrDone,
    }

    let (tx, rx) = mpsc::channel::<Msg>();
    let mut handles = Vec::new();

    if let Some(mut stdout) = child.take_stdout() {
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            use std::io::Read as _;
            let mut buf = [0u8; 4096];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(Msg::Out(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(Msg::OutDone);
        }));
    } else {
        let _ = tx.send(Msg::OutDone);
    }

    if let Some(mut stderr) = child.take_stderr() {
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            use std::io::Read as _;
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(Msg::Err(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(Msg::ErrDone);
        }));
    } else {
        let _ = tx.send(Msg::ErrDone);
    }
    drop(tx);

    let mut done_out = false;
    let mut done_err = false;
    while !(done_out && done_err) {
        match rx.recv() {
            Ok(Msg::Out(b)) => out_sink.write(&b),
            Ok(Msg::Err(b)) => err_sink.write(&b),
            Ok(Msg::OutDone) => done_out = true,
            Ok(Msg::ErrDone) => done_err = true,
            Err(_) => break,
        }
    }

    for h in handles {
        let _ = h.join();
    }
    child.wait().unwrap_or(1)
}
