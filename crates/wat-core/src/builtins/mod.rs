use crate::builtins::resolve::resolve_path;
use crate::context::{Context, LoopCtl};
use crate::io::ShellIo;
use crate::vfs::FileType;

pub mod easter;
#[cfg(feature = "native-pty")]
pub mod jobs_builtins;
pub mod resolve;
pub mod test_cmd;

/// Run a builtin. Returns `Some(exit_code)` if known, `None` if not a builtin.
/// `history` is `None` when called from a context that doesn't track it (e.g., pipeline stage).
pub fn run_builtin<'a>(
    name: &str,
    args: &[String],
    ctx: &mut Context,
    io: &mut ShellIo<'a>,
) -> Option<i32> {
    match name {
        "echo" => Some(echo(args, io)),
        "pwd" => Some(pwd(ctx, io)),
        "cd" => Some(cd(args, ctx, io)),
        "exit" => Some(exit_builtin(args, ctx)),
        "env" => Some(env_builtin(ctx, io)),
        "export" => Some(export(args, ctx, io)),
        "unset" => Some(unset(args, ctx)),
        "help" => Some(easter::help_easter(io)),
        "clear" => Some(clear(io)),
        "true" => Some(0),
        "false" => Some(1),
        "break" => Some(loop_ctl_builtin(LoopCtl::Break, "break", ctx, io)),
        "continue" => Some(loop_ctl_builtin(LoopCtl::Continue, "continue", ctx, io)),
        "test" => Some(test_cmd::test_builtin("test", args, ctx, io)),
        "[" => Some(test_cmd::test_builtin("[", args, ctx, io)),
        "set" => Some(set_builtin(args, ctx, io)),
        "shift" => Some(shift_builtin(args, ctx, io)),
        "return" => Some(return_builtin(args, ctx, io)),
        ":" => Some(0),
        "printf" => Some(printf_builtin(args, io)),
        "read" => Some(read_builtin(args, ctx, io)),
        "eval" => Some(eval_builtin(args, ctx, io)),
        "source" | "." => Some(source_builtin(args, ctx, io)),
        // File builtins
        "ls" => Some(ls(args, ctx, io)),
        "cat" => Some(cat(args, ctx, io)),
        "mkdir" => Some(mkdir_builtin(args, ctx, io)),
        "touch" => Some(touch(args, ctx, io)),
        "rm" => Some(rm(args, ctx, io)),
        "cp" => Some(cp(args, ctx, io)),
        "mv" => Some(mv(args, ctx, io)),
        // Text-processing builtins (use stdin)
        "grep" => Some(grep(args, io)),
        "head" => Some(head(args, io)),
        "tail" => Some(tail(args, io)),
        "wc" => Some(wc(args, io)),
        "sort" => Some(sort_builtin(args, io)),
        "uniq" => Some(uniq_builtin(io)),
        "tr" => Some(tr(args, io)),
        "cut" => Some(cut(args, io)),
        "history" => Some(history_builtin(ctx, io)),
        // Job control builtins (native-pty only)
        #[cfg(feature = "native-pty")]
        "jobs" => Some(jobs_builtins::jobs_builtin(ctx, io)),
        #[cfg(feature = "native-pty")]
        "fg" => Some(jobs_builtins::fg_builtin(args, ctx, io)),
        #[cfg(feature = "native-pty")]
        "bg" => Some(jobs_builtins::bg_builtin(args, ctx, io)),
        #[cfg(feature = "native-pty")]
        "kill" => Some(jobs_builtins::kill_builtin(args, ctx, io)),
        // Easter eggs
        "sudo" => Some(easter::sudo(io)),
        "vim" | "vi" | "nano" | "emacs" => Some(easter::editor_trap(name, io)),
        "sl" => Some(easter::sl(io)),
        "./whoami.sh" | "bash whoami.sh" | "sh whoami.sh" => Some(easter::whoami_sh(io)),
        "__konami__" => Some(easter::konami(io)),
        _ => None,
    }
}

/// Returns true if `name` resolves to a wat builtin. Used by the pipeline
/// executor to decide between the synchronous builtin path and the
/// process-spawning external path.
pub fn is_builtin(name: &str) -> bool {
    #[cfg(feature = "native-pty")]
    if matches!(name, "jobs" | "fg" | "bg" | "kill") {
        return true;
    }
    matches!(
        name,
        "echo"
            | "pwd"
            | "cd"
            | "exit"
            | "env"
            | "export"
            | "unset"
            | "help"
            | "clear"
            | "true"
            | "false"
            | "break"
            | "continue"
            | "test"
            | "["
            | "set"
            | "shift"
            | "return"
            | ":"
            | "printf"
            | "read"
            | "eval"
            | "source"
            | "."
            | "ls"
            | "cat"
            | "mkdir"
            | "touch"
            | "rm"
            | "cp"
            | "mv"
            | "grep"
            | "head"
            | "tail"
            | "wc"
            | "sort"
            | "uniq"
            | "tr"
            | "cut"
            | "history"
            | "sudo"
            | "vim"
            | "vi"
            | "nano"
            | "emacs"
            | "sl"
            | "./whoami.sh"
            | "bash whoami.sh"
            | "sh whoami.sh"
            | "__konami__"
    )
}

/// `set`: with no args, list variables. Leading `-e`/`-u`/`-x` (and their `+`
/// disabling forms, combinable like `-eux`) toggle shell options. A `--`, or
/// the first non-option word, ends option parsing; the remaining words replace
/// the positional parameters. `set -e` (options only, no operands) leaves the
/// positionals untouched, while `set --` clears them.
fn set_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if args.is_empty() {
        return env_builtin(ctx, io);
    }
    let mut i = 0;
    let mut explicit_end = false;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            explicit_end = true;
            i += 1;
            break;
        }
        let enable = match a.as_bytes().first() {
            Some(b'-') => true,
            Some(b'+') => false,
            _ => break, // first operand
        };
        if a.len() == 1 {
            break; // a lone "-"/"+" is not an option
        }
        for c in a[1..].chars() {
            match c {
                'e' => ctx.opt_errexit = enable,
                'u' => ctx.opt_nounset = enable,
                'x' => ctx.opt_xtrace = enable,
                other => {
                    io.write_err(&format!("wat: set: -{}: invalid option\n", other));
                    return 2;
                }
            }
        }
        i += 1;
    }
    // Replace positionals only when operands were given or `--` appeared.
    if explicit_end || i < args.len() {
        ctx.env.params = args[i..].to_vec();
    }
    0
}

/// `return [n]`: stop the current function with status `n` (default `$?`).
/// Only meaningful inside a function; elsewhere it's an error.
fn return_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if ctx.fn_depth == 0 {
        io.write_err("wat: return: can only `return' from a function\n");
        return 1;
    }
    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(ctx.env.last_exit_code);
    ctx.env.last_exit_code = code;
    ctx.returning = Some(code);
    code
}

/// `shift [n]`: drop the first `n` positional parameters (default 1).
fn shift_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let n = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    if n > ctx.env.params.len() {
        io.write_err("wat: shift: shift count out of range\n");
        return 1;
    }
    ctx.env.params.drain(0..n);
    0
}

/// `printf FORMAT [ARG...]`: a focused subset of POSIX printf. Supports the
/// `%s`, `%d`/`%i`, `%x`, and `%%` conversions and the `\n \t \r \\ \0 \a`
/// escapes. When more arguments are given than the format consumes, the format
/// is reused until the arguments are exhausted (POSIX cycling).
fn printf_builtin(args: &[String], io: &mut ShellIo) -> i32 {
    let Some(format) = args.first() else {
        io.write_err("printf: usage: printf format [arguments]\n");
        return 2;
    };
    let rest = &args[1..];
    let mut out = String::new();
    let mut ai = 0;
    loop {
        let (next, had_conversion) = printf_pass(format, rest, ai, &mut out);
        ai = next;
        if !had_conversion || ai >= rest.len() {
            break;
        }
    }
    io.write_out(&out);
    0
}

/// One pass over `fmt`, consuming arguments starting at `start`. Returns the
/// next argument index and whether the format contained an argument-consuming
/// conversion (used to decide whether to cycle).
fn printf_pass(fmt: &str, args: &[String], start: usize, out: &mut String) -> (usize, bool) {
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut ai = start;
    let mut had_conversion = false;
    let int_arg = |args: &[String], idx: usize| -> i64 {
        args.get(idx)
            .map(|s| s.trim().parse::<i64>().unwrap_or(0))
            .unwrap_or(0)
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '\\' => out.push('\\'),
                '0' => out.push('\0'),
                'a' => out.push('\u{7}'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            i += 1;
        } else if c == '%' && i + 1 < chars.len() {
            i += 1;
            match chars[i] {
                '%' => out.push('%'),
                's' => {
                    had_conversion = true;
                    if let Some(a) = args.get(ai) {
                        out.push_str(a);
                    }
                    ai += 1;
                }
                'd' | 'i' => {
                    had_conversion = true;
                    out.push_str(&int_arg(args, ai).to_string());
                    ai += 1;
                }
                'x' => {
                    had_conversion = true;
                    out.push_str(&format!("{:x}", int_arg(args, ai)));
                    ai += 1;
                }
                other => {
                    out.push('%');
                    out.push(other);
                }
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    (ai, had_conversion)
}

/// `read [-r] [NAME...]`: read one line from stdin and split it on whitespace
/// into the named variables (the last name receives any remainder). With no
/// names, the line is stored in `REPLY`. Returns 1 at end of input. `-r` is
/// accepted (backslashes are already taken literally). Reads only the first
/// line of the provided stdin (a pipeline or redirect); the rest is discarded.
fn read_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let mut names: Vec<&String> = Vec::new();
    for a in args {
        if a != "-r" {
            names.push(a);
        }
    }
    let input = io.stdin_str();
    if input.is_empty() {
        return 1; // EOF: nothing read.
    }
    let line = input.lines().next().unwrap_or("");
    let tokens: Vec<&str> = line.split_whitespace().collect();

    if names.is_empty() {
        ctx.env.set("REPLY", line.trim());
        return 0;
    }
    let n = names.len();
    for (idx, name) in names.iter().enumerate() {
        let value = if idx + 1 < n {
            tokens.get(idx).copied().unwrap_or("").to_string()
        } else if idx < tokens.len() {
            tokens[idx..].join(" ")
        } else {
            String::new()
        };
        ctx.env.set((*name).clone(), value);
    }
    0
}

/// `eval ARG...`: join the arguments with spaces, then parse and run the result
/// in the current shell (so any assignments, functions, or `cd` persist).
fn eval_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let src = args.join(" ");
    run_in_current_shell(&src, ctx, io)
}

/// `.`/`source FILE`: read FILE from the VFS and run its contents in the
/// current shell. Positional parameters are left unchanged.
fn source_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let Some(file) = args.first() else {
        io.write_err("wat: source: filename argument required\n");
        return 2;
    };
    let path = resolve_path(file, &ctx.env.cwd);
    let bytes = match ctx.vfs.read(&path) {
        Ok(b) => b,
        Err(e) => {
            io.write_err(&format!("wat: source: {}: {}\n", file, e));
            return 1;
        }
    };
    let src = String::from_utf8_lossy(&bytes).into_owned();
    run_in_current_shell(&src, ctx, io)
}

/// Parse `src` and evaluate it in the current shell, routing output through the
/// builtin's I/O. Shared by `eval` and `source`.
fn run_in_current_shell(src: &str, ctx: &mut Context, io: &mut ShellIo) -> i32 {
    match crate::parser::parse(src) {
        Ok(list) => crate::eval::eval_streaming(&list, ctx, io.stdout, io.stderr),
        Err(e) => {
            io.write_err(&format!("wat: {}\n", e));
            2
        }
    }
}

/// `break` / `continue`: request loop control. Only meaningful inside a loop;
/// outside one it prints a diagnostic and is a no-op (exit 0), matching bash.
fn loop_ctl_builtin(ctl: LoopCtl, name: &str, ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if ctx.loop_depth == 0 {
        io.write_err(&format!(
            "wat: {}: only meaningful in a `for', `while', or `until' loop\n",
            name
        ));
        return 0;
    }
    ctx.loop_ctl = Some(ctl);
    0
}

fn history_builtin(ctx: &Context, io: &mut ShellIo) -> i32 {
    for (i, cmd) in ctx.history.iter().enumerate() {
        io.write_out(&format!("{:4}  {}\n", i + 1, cmd));
    }
    0
}

// ── Non-file builtins ──────────────────────────────────────────────────────

fn echo(args: &[String], io: &mut ShellIo) -> i32 {
    let (no_newline, words) = if args.first().map(|s| s.as_str()) == Some("-n") {
        (true, &args[1..])
    } else {
        (false, args)
    };
    io.write_out(&words.join(" "));
    if !no_newline {
        io.write_out("\n");
    }
    0
}

fn pwd(ctx: &Context, io: &mut ShellIo) -> i32 {
    io.write_out(&ctx.env.cwd);
    io.write_out("\n");
    0
}

fn cd(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let target = match args.first() {
        Some(p) => p.clone(),
        None => ctx.env.home().to_string(),
    };

    let target = if target == "~" {
        ctx.env.home().to_string()
    } else if target.starts_with("~/") {
        format!("{}{}", ctx.env.home(), &target[1..])
    } else if target == "-" {
        ctx.env.get("OLDPWD").unwrap_or("/").to_string()
    } else {
        target
    };

    let new_cwd = resolve_path(&target, &ctx.env.cwd);

    if !ctx.vfs.is_dir(&new_cwd) {
        io.write_err(&format!("cd: {}: No such file or directory\n", new_cwd));
        return 1;
    }

    let old = ctx.env.cwd.clone();
    ctx.env.set("OLDPWD", &old);
    ctx.env.cwd = new_cwd.clone();
    ctx.env.set("PWD", &new_cwd);
    0
}

fn exit_builtin(args: &[String], ctx: &mut Context) -> i32 {
    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(ctx.env.last_exit_code);
    ctx.env.last_exit_code = code;
    // Signal the evaluator to stop the current list (so `exit` mid-script
    // terminates immediately); `Shell::feed*` surfaces this as exit_requested.
    ctx.exit_status = Some(code);
    code
}

fn env_builtin(ctx: &Context, io: &mut ShellIo) -> i32 {
    let mut pairs: Vec<String> = ctx
        .env
        .vars()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    pairs.sort();
    for pair in pairs {
        io.write_out(&pair);
        io.write_out("\n");
    }
    0
}

fn export(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if args.is_empty() {
        return env_builtin(ctx, io);
    }
    for arg in args {
        if let Some((k, v)) = arg.split_once('=') {
            ctx.env.set(k, v);
        }
    }
    0
}

fn unset(args: &[String], ctx: &mut Context) -> i32 {
    for arg in args {
        ctx.env.unset(arg);
    }
    0
}

fn clear(io: &mut ShellIo) -> i32 {
    io.write_out("\x1b[2J\x1b[H");
    0
}

// ── File builtins ──────────────────────────────────────────────────────────

fn ls(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let show_hidden = args.iter().any(|a| a == "-a" || a == "-la" || a == "-al");
    let long = args.iter().any(|a| a == "-l" || a == "-la" || a == "-al");
    let path = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| resolve_path(s, &ctx.env.cwd))
        .unwrap_or_else(|| ctx.env.cwd.clone());

    match ctx.vfs.list(&path) {
        Ok(mut entries) => {
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            for entry in &entries {
                if !show_hidden && entry.name.starts_with('.') {
                    continue;
                }
                if long {
                    let kind = match entry.file_type {
                        FileType::Dir => "d",
                        FileType::File => "-",
                    };
                    io.write_out(&format!("{} {}\n", kind, entry.name));
                } else {
                    io.write_out(&entry.name);
                    io.write_out("\n");
                }
            }
            0
        }
        Err(e) => {
            io.write_err(&format!("ls: {}\n", e));
            1
        }
    }
}

fn cat(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let file_args: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if file_args.is_empty() {
        // cat with no args: copy stdin to stdout
        let data = io.stdin.to_vec();
        io.write_out_bytes(&data);
        return 0;
    }
    let mut code = 0;
    for arg in file_args {
        let path = resolve_path(arg, &ctx.env.cwd);
        match ctx.vfs.read(&path) {
            Ok(content) => {
                io.write_out_bytes(&content);
            }
            Err(e) => {
                io.write_err(&format!("cat: {}\n", e));
                code = 1;
            }
        }
    }
    code
}

fn mkdir_builtin(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if args.is_empty() {
        io.write_err("mkdir: missing operand\n");
        return 1;
    }
    let mut code = 0;
    for arg in args.iter().filter(|a| !a.starts_with('-')) {
        let path = resolve_path(arg, &ctx.env.cwd);
        if let Err(e) = ctx.vfs.mkdir(&path) {
            io.write_err(&format!("mkdir: {}\n", e));
            code = 1;
        }
    }
    code
}

fn touch(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    if args.is_empty() {
        io.write_err("touch: missing operand\n");
        return 1;
    }
    let mut code = 0;
    for arg in args {
        let path = resolve_path(arg, &ctx.env.cwd);
        if !ctx.vfs.exists(&path) {
            if let Err(e) = ctx.vfs.write(&path, b"") {
                io.write_err(&format!("touch: {}\n", e));
                code = 1;
            }
        }
    }
    code
}

fn rm(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let flags: String = args
        .iter()
        .filter(|a| a.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>()
        .concat();
    let recursive = flags.contains('r') || flags.contains('R');
    let force = flags.contains('f');

    let paths: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(|a| resolve_path(a, &ctx.env.cwd))
        .collect();

    if paths.is_empty() {
        io.write_err("rm: missing operand\n");
        return 1;
    }

    for path in &paths {
        if path == "/" || path == "/*" {
            io.write_out("rm: nice try. the void stares back, but your filesystem does not.\n");
            return 1;
        }
    }

    let mut code = 0;
    for path in &paths {
        if let Err(e) = ctx.vfs.remove(path, recursive) {
            if !force {
                io.write_err(&format!("rm: {}\n", e));
                code = 1;
            }
        }
    }
    code
}

fn cp(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let non_flags: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if non_flags.len() < 2 {
        io.write_err("cp: missing operand\n");
        return 1;
    }
    let src = resolve_path(non_flags[0], &ctx.env.cwd);
    let dst = resolve_path(non_flags[1], &ctx.env.cwd);
    match ctx.vfs.copy(&src, &dst) {
        Ok(()) => 0,
        Err(e) => {
            io.write_err(&format!("cp: {}\n", e));
            1
        }
    }
}

fn mv(args: &[String], ctx: &mut Context, io: &mut ShellIo) -> i32 {
    let non_flags: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if non_flags.len() < 2 {
        io.write_err("mv: missing operand\n");
        return 1;
    }
    let src = resolve_path(non_flags[0], &ctx.env.cwd);
    let dst = resolve_path(non_flags[1], &ctx.env.cwd);
    match ctx.vfs.rename(&src, &dst) {
        Ok(()) => 0,
        Err(e) => {
            io.write_err(&format!("mv: {}\n", e));
            1
        }
    }
}

// ── Text-processing builtins ───────────────────────────────────────────────

fn grep(args: &[String], io: &mut ShellIo) -> i32 {
    let pattern = match args.first() {
        Some(p) => p.as_str(),
        None => {
            io.write_err("grep: missing pattern\n");
            return 1;
        }
    };
    let input = io.stdin_str().to_string();
    let mut matched = false;
    for line in input.lines() {
        if line.contains(pattern) {
            io.write_out(line);
            io.write_out("\n");
            matched = true;
        }
    }
    if matched {
        0
    } else {
        1
    }
}

fn head(args: &[String], io: &mut ShellIo) -> i32 {
    let n = parse_n_flag(args, 10);
    let input = io.stdin_str().to_string();
    for line in input.lines().take(n) {
        io.write_out(line);
        io.write_out("\n");
    }
    0
}

fn tail(args: &[String], io: &mut ShellIo) -> i32 {
    let n = parse_n_flag(args, 10);
    let input = io.stdin_str().to_string();
    let lines: Vec<&str> = input.lines().collect();
    let start = lines.len().saturating_sub(n);
    for line in &lines[start..] {
        io.write_out(line);
        io.write_out("\n");
    }
    0
}

fn wc(args: &[String], io: &mut ShellIo) -> i32 {
    let count_lines = args.iter().any(|a| a == "-l");
    let count_words = args.iter().any(|a| a == "-w");
    let count_chars = args.iter().any(|a| a == "-c");
    let all = !count_lines && !count_words && !count_chars;
    let input = io.stdin_str().to_string();
    let lines = input.lines().count();
    let words = input.split_whitespace().count();
    let chars = input.len();
    if all {
        io.write_out(&format!("{} {} {}\n", lines, words, chars));
    } else {
        let mut parts = Vec::new();
        if count_lines {
            parts.push(lines.to_string());
        }
        if count_words {
            parts.push(words.to_string());
        }
        if count_chars {
            parts.push(chars.to_string());
        }
        io.write_out(&parts.join(" "));
        io.write_out("\n");
    }
    0
}

fn sort_builtin(args: &[String], io: &mut ShellIo) -> i32 {
    let reverse = args.iter().any(|a| a == "-r");
    let input = io.stdin_str().to_string();
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_unstable();
    if reverse {
        lines.reverse();
    }
    for line in lines {
        io.write_out(line);
        io.write_out("\n");
    }
    0
}

fn uniq_builtin(io: &mut ShellIo) -> i32 {
    let input = io.stdin_str().to_string();
    let mut prev: Option<&str> = None;
    for line in input.lines() {
        if prev != Some(line) {
            io.write_out(line);
            io.write_out("\n");
            prev = Some(line);
        }
    }
    0
}

fn tr(args: &[String], io: &mut ShellIo) -> i32 {
    if args.len() < 2 {
        io.write_err("tr: missing operand\n");
        return 1;
    }
    let from: Vec<char> = args[0].chars().collect();
    let to: Vec<char> = args[1].chars().collect();
    let input = io.stdin_str().to_string();
    let out: String = input
        .chars()
        .map(|c| {
            if let Some(pos) = from.iter().position(|&f| f == c) {
                *to.get(pos).unwrap_or(&c)
            } else {
                c
            }
        })
        .collect();
    io.write_out(&out);
    0
}

fn cut(args: &[String], io: &mut ShellIo) -> i32 {
    let delim = args
        .windows(2)
        .find(|w| w[0] == "-d")
        .and_then(|w| w[1].chars().next())
        .unwrap_or('\t');
    let field = args
        .windows(2)
        .find(|w| w[0] == "-f")
        .and_then(|w| w[1].parse::<usize>().ok())
        .unwrap_or(1);
    let input = io.stdin_str().to_string();
    for line in input.lines() {
        let parts: Vec<&str> = line.splitn(field + 1, delim).collect();
        if let Some(part) = parts.get(field - 1) {
            io.write_out(part);
            io.write_out("\n");
        }
    }
    0
}

fn parse_n_flag(args: &[String], default: usize) -> usize {
    args.windows(2)
        .find(|w| w[0] == "-n")
        .and_then(|w| w[1].parse().ok())
        .or_else(|| {
            args.iter()
                .find(|a| a.starts_with("-n") && a.len() > 2)
                .and_then(|a| a[2..].parse().ok())
        })
        .unwrap_or(default)
}
