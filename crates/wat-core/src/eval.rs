use crate::ast::{Command, CompoundCommand, List, Pipeline, Redirect, Separator, SimpleCommand};
use crate::builtins::resolve::resolve_path;
use crate::builtins::run_builtin;
use crate::context::{Context, LoopCtl};
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

        // `set -e` (errexit): a non-zero command terminates the shell, except
        // inside a condition context or as an operand of `&&`/`||` (where the
        // status is "tested"). Detected here by the trailing separator.
        if ctx.opt_errexit
            && !ctx.errexit_suppressed
            && code != 0
            && matches!(
                sep,
                Separator::Semi | Separator::End | Separator::Background
            )
        {
            ctx.exit_status = Some(code);
        }

        // A pending `break`/`continue`, `return`, or `exit` stops the rest of
        // this list so it can propagate to the enclosing loop / function / shell.
        if ctx.loop_ctl.is_some() || ctx.returning.is_some() || ctx.exit_status.is_some() {
            break;
        }

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
            // Background (`&`) is handled by the REPL before reaching eval.
            Separator::Semi | Separator::End | Separator::Background => {}
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

/// Evaluate `src` as a command list, capturing its **stdout** as bytes and
/// returning `(exit_code, stdout)`. Stderr is forwarded to `err` so a
/// substitution's diagnostics still reach the terminal. Used by command
/// substitution (`$(...)`, backticks) in `expand::expand_word_ctx`.
pub fn eval_capture_stdout(
    src: &str,
    ctx: &mut Context,
    err: &mut dyn OutputSink,
) -> (i32, Vec<u8>) {
    use crate::parser::parse;
    match parse(src) {
        Ok(list) => {
            let mut out = VecSink::new();
            // An `exit` inside a command substitution must not terminate the
            // parent shell (no subshell, so isolate the flag here).
            let saved_exit = ctx.exit_status.take();
            let code = eval_streaming(&list, ctx, &mut out, err);
            ctx.exit_status = saved_exit;
            (code, out.into_inner())
        }
        Err(e) => {
            err.write(format!("wat: {}\n", e).as_bytes());
            (2, Vec::new())
        }
    }
}

/// Expand a command's name and arguments on the command path — variable,
/// tilde, and command substitution — then glob each result. Substitution
/// stderr is forwarded to `err`. Phase B: each source word yields exactly one
/// expanded word (no field splitting yet); the name is the first such word.
fn expand_command_words(
    cmd: &SimpleCommand,
    ctx: &mut Context,
    err: &mut dyn OutputSink,
) -> (String, Vec<String>) {
    let name = crate::expand::expand_word_ctx(&cmd.name, ctx, err)
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut args = Vec::new();
    for a in &cmd.args {
        for w in crate::expand::expand_word_ctx(a, ctx, err) {
            args.extend(glob_expand(&w, ctx.vfs.as_ref(), &ctx.env.cwd));
        }
    }
    (name, args)
}

/// Run a pipeline. Single-command pipelines go through the fast path that
/// also handles redirects. Multi-command pipelines route through
/// `eval_pipeline_chained`, which chains consecutive external segments via
/// `ChildStdin::Pipe` so they stream without parent buffering.
fn eval_pipeline(
    pipeline: &Pipeline,
    ctx: &mut Context,
    initial_stdin: &[u8],
    final_out: &mut dyn OutputSink,
    final_err: &mut dyn OutputSink,
) -> i32 {
    if pipeline.0.len() == 1 {
        return eval_command(&pipeline.0[0], ctx, initial_stdin, final_out, final_err);
    }
    eval_pipeline_chained(pipeline, ctx, initial_stdin, final_out, final_err)
}

/// Evaluate a single command (simple, compound, or function definition),
/// outside the multi-segment pipeline machinery.
fn eval_command(
    cmd: &Command,
    ctx: &mut Context,
    stdin: &[u8],
    out: &mut dyn OutputSink,
    err: &mut dyn OutputSink,
) -> i32 {
    match cmd {
        Command::Simple(sc) => run_command(sc, ctx, stdin, out, err),
        Command::Compound(cc) => eval_compound(cc, ctx, out, err),
        Command::FunctionDef { name, body } => {
            ctx.functions
                .insert(name.clone(), std::rc::Rc::new((**body).clone()));
            ctx.env.last_exit_code = 0;
            0
        }
    }
}

/// Evaluate a compound (control-flow) command. Runs in the current shell — no
/// subshell — so assignments and `cd` inside it persist.
fn eval_compound(
    cc: &CompoundCommand,
    ctx: &mut Context,
    out: &mut dyn OutputSink,
    err: &mut dyn OutputSink,
) -> i32 {
    match cc {
        CompoundCommand::If {
            branches,
            else_body,
        } => {
            for (cond, body) in branches {
                let cond_code = eval_condition(cond, ctx, out, err);
                ctx.env.last_exit_code = cond_code;
                if cond_code == 0 {
                    return eval_streaming(body, ctx, out, err);
                }
            }
            match else_body {
                Some(body) => eval_streaming(body, ctx, out, err),
                None => 0,
            }
        }
        CompoundCommand::While { cond, body } => eval_while(cond, body, false, ctx, out, err),
        CompoundCommand::Until { cond, body } => eval_while(cond, body, true, ctx, out, err),
        CompoundCommand::For { var, words, body } => eval_for(var, words, body, ctx, out, err),
        CompoundCommand::BraceGroup(body) => eval_streaming(body, ctx, out, err),
        CompoundCommand::Case { word, arms } => {
            // Expand the subject (no field splitting), then run the first arm
            // whose any pattern glob-matches it.
            let subject = crate::expand::expand_value(word, ctx, err);
            for arm in arms {
                let matched = arm.patterns.iter().any(|p| {
                    let pat = crate::expand::expand_value(p, ctx, err);
                    crate::glob::match_glob(&pat, &subject)
                });
                if matched {
                    return eval_streaming(&arm.body, ctx, out, err);
                }
            }
            0
        }
    }
}

/// Evaluate an `if`/`while`/`until` condition list with `errexit` suppressed:
/// a failing condition tests false, it does not abort the shell.
fn eval_condition(
    cond: &List,
    ctx: &mut Context,
    out: &mut dyn OutputSink,
    err: &mut dyn OutputSink,
) -> i32 {
    let saved = ctx.errexit_suppressed;
    ctx.errexit_suppressed = true;
    let code = eval_streaming(cond, ctx, out, err);
    ctx.errexit_suppressed = saved;
    code
}

/// `while`/`until` loop. `negate` flips the condition sense for `until`.
fn eval_while(
    cond: &List,
    body: &List,
    negate: bool,
    ctx: &mut Context,
    out: &mut dyn OutputSink,
    err: &mut dyn OutputSink,
) -> i32 {
    use std::sync::atomic::Ordering;
    ctx.loop_depth += 1;
    let mut code = 0;
    loop {
        if ctx.cancel.load(Ordering::Relaxed) {
            break;
        }
        let cond_code = eval_condition(cond, ctx, out, err);
        ctx.env.last_exit_code = cond_code;
        let run = if negate {
            cond_code != 0
        } else {
            cond_code == 0
        };
        if !run {
            break;
        }
        code = eval_streaming(body, ctx, out, err);
        match ctx.loop_ctl.take() {
            Some(LoopCtl::Break) => break,
            Some(LoopCtl::Continue) => continue,
            None => {}
        }
    }
    ctx.loop_depth -= 1;
    code
}

/// `for var in words; do body; done`.
fn eval_for(
    var: &str,
    words: &[String],
    body: &List,
    ctx: &mut Context,
    out: &mut dyn OutputSink,
    err: &mut dyn OutputSink,
) -> i32 {
    use std::sync::atomic::Ordering;
    // Expand the word list the same way command arguments are expanded:
    // variable/command/arith substitution + field splitting + globbing.
    let mut items: Vec<String> = Vec::new();
    for w in words {
        for e in crate::expand::expand_word_ctx(w, ctx, err) {
            items.extend(glob_expand(&e, ctx.vfs.as_ref(), &ctx.env.cwd));
        }
    }

    ctx.loop_depth += 1;
    let mut code = 0;
    for item in items {
        if ctx.cancel.load(Ordering::Relaxed) {
            break;
        }
        ctx.env.set(var.to_string(), item);
        code = eval_streaming(body, ctx, out, err);
        match ctx.loop_ctl.take() {
            Some(LoopCtl::Break) => break,
            Some(LoopCtl::Continue) => continue,
            None => {}
        }
    }
    ctx.loop_depth -= 1;
    code
}

/// Multi-segment pipeline executor. For each adjacent pair of external
/// segments, the upstream's stdout is fed into the downstream's stdin via
/// `ChildStdin::Pipe`, so the parent never buffers the whole stream.
/// Builtins still go through the synchronous buffered path; at a
/// builtin↔external boundary we materialize the buffer (builtin→external) or
/// fully drain the upstream child (external→builtin).
fn eval_pipeline_chained(
    pipeline: &Pipeline,
    ctx: &mut Context,
    initial_stdin: &[u8],
    final_out: &mut dyn OutputSink,
    final_err: &mut dyn OutputSink,
) -> i32 {
    enum PipelineStdin {
        Bytes(Vec<u8>),
        #[cfg(feature = "native-proc")]
        Reader(Box<dyn std::io::Read + Send>),
    }

    // Apply any segment assignment prefixes transiently for the whole pipeline
    // and restore them before returning. Strict POSIX scopes these per segment;
    // assignment prefixes on a pipeline segment are rare, so we apply them
    // across the pipeline (and they do affect later segments' env here).
    let saved_assignments: Vec<(String, Option<String>)> = {
        let mut saved = Vec::new();
        for cmd in &pipeline.0 {
            if let Command::Simple(sc) = cmd {
                for (key, raw) in &sc.assignments {
                    let val = crate::expand::expand_value(raw, ctx, final_err);
                    saved.push((key.clone(), ctx.env.get(key).map(|s| s.to_string())));
                    ctx.env.set(key.clone(), val);
                }
            }
        }
        saved
    };

    let n = pipeline.0.len();
    let mut current: PipelineStdin = PipelineStdin::Bytes(initial_stdin.to_vec());
    let mut last_code = 0i32;
    #[cfg(feature = "native-proc")]
    let mut pending_children: Vec<Box<dyn crate::process::ChildProcess>> = Vec::new();
    // Background threads draining mid-pipeline stderrs into per-segment
    // buffers. We can't pass `&mut dyn OutputSink` across threads (not Send),
    // so each thread fills its own Vec and we flush them into `final_err` at
    // the very end, after the pipeline has fully wound down.
    #[cfg(feature = "native-proc")]
    let mut pending_stderr_handles: Vec<std::thread::JoinHandle<Vec<u8>>> = Vec::new();
    #[cfg(feature = "native-proc")]
    let cancel_flag = ctx.cancel.clone();

    for (idx, command) in pipeline.0.iter().enumerate() {
        let is_last = idx + 1 == n;

        // A compound command as a pipeline segment (`if ...; fi | cmd`) runs
        // synchronously and buffers its stdout for the next segment, like a
        // builtin. It does not consume the piped stdin (rare; documented).
        let cmd = match command {
            Command::Simple(sc) => sc,
            // A compound command or function definition as a pipeline segment
            // runs synchronously and buffers its stdout for the next segment.
            Command::Compound(_) | Command::FunctionDef { .. } => {
                let mut buffered_out = VecSink::new();
                let code = {
                    let stdout_target: &mut dyn OutputSink = if is_last {
                        final_out
                    } else {
                        &mut buffered_out
                    };
                    let stdin_bytes =
                        match std::mem::replace(&mut current, PipelineStdin::Bytes(Vec::new())) {
                            PipelineStdin::Bytes(b) => b,
                            #[cfg(feature = "native-proc")]
                            PipelineStdin::Reader(mut r) => {
                                let mut b = Vec::new();
                                use std::io::Read as _;
                                let _ = r.read_to_end(&mut b);
                                b
                            }
                        };
                    eval_command(command, ctx, &stdin_bytes, stdout_target, final_err)
                };
                last_code = code;
                ctx.env.last_exit_code = code;
                current = PipelineStdin::Bytes(buffered_out.into_inner());
                continue;
            }
        };

        // Resolve name + args once; needed for both builtin lookup and
        // external spawn. Command substitution writes its stderr to final_err.
        let (name, args) = expand_command_words(cmd, ctx, final_err);
        let name = normalize_easter_egg(&name, &args);

        let is_builtin = crate::builtins::is_builtin(&name);

        if is_builtin {
            // Builtins consume bytes; materialize a Reader if we have one.
            let stdin_bytes = match current {
                PipelineStdin::Bytes(b) => b,
                #[cfg(feature = "native-proc")]
                PipelineStdin::Reader(mut r) => {
                    let mut b = Vec::new();
                    use std::io::Read as _;
                    let _ = r.read_to_end(&mut b);
                    b
                }
            };
            // Apply input redirect override if present.
            let stdin_bytes = apply_input_redirect(cmd, ctx, stdin_bytes);
            let has_out_redirect = cmd
                .redirects
                .iter()
                .any(|r| matches!(r, Redirect::Out(_) | Redirect::Append(_)));
            let has_err_redirect = cmd.redirects.iter().any(|r| matches!(r, Redirect::Err(_)));

            let mut local_out = VecSink::new();
            let mut local_err = VecSink::new();
            let mut buffered_out = VecSink::new();
            let code = {
                let stdout_target: &mut dyn OutputSink = if has_out_redirect {
                    &mut local_out
                } else if is_last {
                    final_out
                } else {
                    &mut buffered_out
                };
                let stderr_target: &mut dyn OutputSink = if has_err_redirect {
                    &mut local_err
                } else {
                    final_err
                };
                let mut io = ShellIo {
                    stdin: &stdin_bytes,
                    stdout: stdout_target,
                    stderr: stderr_target,
                };
                run_builtin(&name, &args, ctx, &mut io).unwrap_or_else(|| {
                    io.write_err(&format!("wat: command not found: {}\n", name));
                    127
                })
            };
            apply_output_redirects(cmd, ctx, local_out.as_slice(), local_err.as_slice());
            last_code = code;
            ctx.env.last_exit_code = code;
            current = PipelineStdin::Bytes(buffered_out.into_inner());
            continue;
        }

        // External segment. Behavior depends on whether native-proc is
        // compiled in.
        #[cfg(feature = "native-proc")]
        {
            // Apply input redirect by materializing the reader to bytes.
            let stdin = match current {
                PipelineStdin::Bytes(b) => ChildStdin::Bytes(apply_input_redirect(cmd, ctx, b)),
                PipelineStdin::Reader(r) => {
                    if cmd.redirects.iter().any(|x| matches!(x, Redirect::In(_))) {
                        // Input redirect overrides upstream pipe.
                        ChildStdin::Bytes(apply_input_redirect(cmd, ctx, Vec::new()))
                    } else {
                        ChildStdin::Pipe(r)
                    }
                }
            };
            let has_out_redirect = cmd
                .redirects
                .iter()
                .any(|r| matches!(r, Redirect::Out(_) | Redirect::Append(_)));
            let has_err_redirect = cmd.redirects.iter().any(|r| matches!(r, Redirect::Err(_)));

            let path = match ctx.process_host.lookup(&name) {
                Some(p) => p,
                None => {
                    let msg = format!("wat: command not found: {}\n", name);
                    final_err.write(msg.as_bytes());
                    last_code = 127;
                    ctx.env.last_exit_code = last_code;
                    current = PipelineStdin::Bytes(Vec::new());
                    continue;
                }
            };

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

            let mut child = match ctx.process_host.spawn(spec, stdin) {
                Ok(c) => c,
                Err(ProcessError::Unsupported) => {
                    let msg = format!("wat: command not found: {}\n", name);
                    final_err.write(msg.as_bytes());
                    last_code = 127;
                    ctx.env.last_exit_code = last_code;
                    current = PipelineStdin::Bytes(Vec::new());
                    continue;
                }
                Err(ProcessError::Io(e)) => {
                    let msg = format!("wat: {}: {}\n", name, e);
                    final_err.write(msg.as_bytes());
                    last_code = 126;
                    ctx.env.last_exit_code = last_code;
                    current = PipelineStdin::Bytes(Vec::new());
                    continue;
                }
            };

            if is_last {
                // Drain to final_out/final_err (or to local sinks if
                // redirected) and wait.
                let mut local_out = VecSink::new();
                let mut local_err = VecSink::new();
                let code = {
                    let out_target: &mut dyn OutputSink = if has_out_redirect {
                        &mut local_out
                    } else {
                        final_out
                    };
                    let err_target: &mut dyn OutputSink = if has_err_redirect {
                        &mut local_err
                    } else {
                        final_err
                    };
                    stream_child(&mut *child, out_target, err_target, &cancel_flag)
                };
                apply_output_redirects(cmd, ctx, local_out.as_slice(), local_err.as_slice());
                last_code = code;
                ctx.env.last_exit_code = code;
                current = PipelineStdin::Bytes(Vec::new());
            } else {
                // Drain stderr in a background thread into a per-segment
                // buffer; we flush these into `final_err` after the pipeline
                // completes. This is critical: doing a synchronous
                // `read_to_end` here would deadlock for producers like `yes`
                // that never EOF until SIGPIPE'd by the downstream — which
                // can't happen until we move on and spawn the downstream.
                if let Some(mut stderr) = child.take_stderr() {
                    pending_stderr_handles.push(std::thread::spawn(move || {
                        use std::io::Read as _;
                        let mut buf = Vec::new();
                        let _ = stderr.read_to_end(&mut buf);
                        buf
                    }));
                }
                if has_out_redirect {
                    // Drain stdout to bytes, write to file, current = empty.
                    let mut buf = Vec::new();
                    if let Some(mut stdout) = child.take_stdout() {
                        use std::io::Read as _;
                        let _ = stdout.read_to_end(&mut buf);
                    }
                    apply_output_redirects(cmd, ctx, &buf, &[]);
                    current = PipelineStdin::Bytes(Vec::new());
                    pending_children.push(child);
                } else {
                    let reader = child
                        .take_stdout()
                        .unwrap_or_else(|| Box::new(std::io::empty()));
                    current = PipelineStdin::Reader(reader);
                    pending_children.push(child);
                }
            }
        }
        #[cfg(not(feature = "native-proc"))]
        {
            // WASM / no-process build: any non-builtin in a pipeline behaves
            // like POSIX with command-not-found — emit the error and pass
            // empty bytes downstream.
            let msg = format!("wat: command not found: {}\n", name);
            final_err.write(msg.as_bytes());
            last_code = 127;
            ctx.env.last_exit_code = last_code;
            current = PipelineStdin::Bytes(Vec::new());
            // Touch `current` to silence the unused variant warning when not
            // compiled with native-proc.
            let _ = is_last;
        }
    }

    // Reap any externals still alive (their stdout/stderr is already drained
    // via the chain or via the background stderr thread).
    #[cfg(feature = "native-proc")]
    for mut c in pending_children {
        let _ = c.wait();
    }
    // Now collect the background stderr buffers and flush them into the
    // final sink in source order.
    #[cfg(feature = "native-proc")]
    for h in pending_stderr_handles {
        if let Ok(buf) = h.join() {
            if !buf.is_empty() {
                final_err.write(&buf);
            }
        }
    }

    for (key, old) in saved_assignments {
        match old {
            Some(v) => ctx.env.set(key, v),
            None => ctx.env.unset(&key),
        }
    }

    last_code
}

fn apply_input_redirect(cmd: &SimpleCommand, ctx: &Context, fallback: Vec<u8>) -> Vec<u8> {
    cmd.redirects
        .iter()
        .find_map(|r| {
            if let Redirect::In(path) = r {
                let full = resolve_path(path, &ctx.env.cwd);
                ctx.vfs.read(&full).ok()
            } else {
                None
            }
        })
        .unwrap_or(fallback)
}

fn apply_output_redirects(
    cmd: &SimpleCommand,
    ctx: &mut Context,
    out_bytes: &[u8],
    err_bytes: &[u8],
) {
    for redirect in &cmd.redirects {
        match redirect {
            Redirect::Out(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let _ = ctx.vfs.write(&full, out_bytes);
            }
            Redirect::Append(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let mut existing = ctx.vfs.read(&full).unwrap_or_default();
                existing.extend_from_slice(out_bytes);
                let _ = ctx.vfs.write(&full, &existing);
            }
            Redirect::Err(path) => {
                let full = resolve_path(path, &ctx.env.cwd);
                let _ = ctx.vfs.write(&full, err_bytes);
            }
            Redirect::In(_) => {}
        }
    }
}

fn normalize_easter_egg(name: &str, args: &[String]) -> String {
    if (name == "bash" || name == "sh") && args.first().map(|s| s.as_str()) == Some("whoami.sh") {
        "bash whoami.sh".to_string()
    } else {
        name.to_string()
    }
}

/// Run a single command, handling redirects and writing output to the supplied
/// sinks. If the command has any output redirects, output is buffered locally
/// so it can be routed to the VFS instead of the outer sinks.
fn run_command(
    cmd: &SimpleCommand,
    ctx: &mut Context,
    stdin_data: &[u8],
    out_sink: &mut dyn OutputSink,
    err_sink: &mut dyn OutputSink,
) -> i32 {
    // Expand name + args with the *current* env first; an assignment prefix
    // must not affect expansion of the rest of the command line (POSIX).
    let (name, args) = expand_command_words(cmd, ctx, err_sink);
    let name = normalize_easter_egg(&name, &args);

    // `set -u`: expansion of an unset variable set `exit_status`; abort the
    // command before running it (and the enclosing list breaks).
    if let Some(code) = ctx.exit_status {
        return code;
    }

    // `set -x` (xtrace): echo the command (name + args) to stderr first.
    if ctx.opt_xtrace && !name.is_empty() {
        let mut line = String::from("+ ");
        line.push_str(&name);
        for a in &args {
            line.push(' ');
            line.push_str(a);
        }
        line.push('\n');
        err_sink.write(line.as_bytes());
    }

    // Pure assignment statement (`x=value ...` with no command word): apply to
    // the shell env. Exit status is 0, or the status of the last command
    // substitution that ran while expanding the values.
    if name.is_empty() {
        ctx.env.last_exit_code = 0;
        for (key, raw) in &cmd.assignments {
            let val = crate::expand::expand_value(raw, ctx, err_sink);
            ctx.env.set(key.clone(), val);
        }
        return ctx.env.last_exit_code;
    }

    // Transient assignment prefix (`x=value cmd ...`): apply to this command's
    // environment only, then restore after it runs. Externals inherit the
    // values via the env snapshot taken at spawn; builtins see them live.
    let saved_assignments: Vec<(String, Option<String>)> = if cmd.assignments.is_empty() {
        Vec::new()
    } else {
        let mut saved = Vec::with_capacity(cmd.assignments.len());
        for (key, raw) in &cmd.assignments {
            let val = crate::expand::expand_value(raw, ctx, err_sink);
            saved.push((key.clone(), ctx.env.get(key).map(|s| s.to_string())));
            ctx.env.set(key.clone(), val);
        }
        saved
    };

    let stdin_bytes = apply_input_redirect(cmd, ctx, stdin_data.to_vec());

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

    apply_output_redirects(cmd, ctx, local_out.as_slice(), local_err.as_slice());

    // Restore any transient assignment-prefix variables.
    for (key, old) in saved_assignments {
        match old {
            Some(v) => ctx.env.set(key, v),
            None => ctx.env.unset(&key),
        }
    }
    code
}

/// Try builtin first; if it doesn't match, ask the ProcessHost; if that
/// doesn't resolve either, emit "command not found".
/// Maximum function-call nesting before refusing to recurse (stack guard).
const MAX_FN_DEPTH: u32 = 512;

fn run_one(
    name: &str,
    args: &[String],
    ctx: &mut Context,
    stdin_bytes: &[u8],
    out_sink: &mut dyn OutputSink,
    err_sink: &mut dyn OutputSink,
) -> i32 {
    // A defined function shadows builtins and externals. Call it with the
    // arguments swapped in as positional parameters; restore them after.
    if let Some(body) = ctx.functions.get(name).cloned() {
        if ctx.fn_depth >= MAX_FN_DEPTH {
            err_sink.write(b"wat: function recursion too deep\n");
            return 1;
        }
        let saved = std::mem::replace(&mut ctx.env.params, args.to_vec());
        ctx.fn_depth += 1;
        let code = eval_command(body.as_ref(), ctx, stdin_bytes, out_sink, err_sink);
        let ret = ctx.returning.take();
        ctx.fn_depth -= 1;
        ctx.env.params = saved;
        return ret.unwrap_or(code);
    }
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
            let cancel = ctx.cancel.clone();
            return match ctx.process_host.spawn(spec, stdin) {
                Ok(mut child) => stream_child(&mut *child, out_sink, err_sink, &cancel),
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
    cancel: &std::sync::atomic::AtomicBool,
) -> i32 {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

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
    let mut signaled = false;
    while !(done_out && done_err) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Msg::Out(b)) => out_sink.write(&b),
            Ok(Msg::Err(b)) => err_sink.write(&b),
            Ok(Msg::OutDone) => done_out = true,
            Ok(Msg::ErrDone) => done_err = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No data right now — poll the cancel flag. If the user hit
                // Ctrl-C, forward SIGINT to the child and keep draining
                // until its pipes close. Only signal once per pipeline so we
                // don't spam the child if it's slow to die.
                if !signaled && cancel.load(Ordering::Relaxed) {
                    let _ = child.signal(crate::process::Signal::Interrupt);
                    signaled = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    for h in handles {
        let _ = h.join();
    }
    child.wait().unwrap_or(1)
}
