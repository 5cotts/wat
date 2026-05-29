use crate::builtins::resolve::resolve_path;
use crate::context::Context;
use crate::vfs::VfsError;

pub mod resolve;

/// Run a builtin. Returns `Some(exit_code)` if known, `None` if not a builtin.
pub fn run_builtin(name: &str, args: &[String], ctx: &mut Context, out: &mut String) -> Option<i32> {
    match name {
        "echo" => Some(echo(args, out)),
        "pwd" => Some(pwd(ctx, out)),
        "cd" => Some(cd(args, ctx, out)),
        "exit" => Some(exit_builtin(args, ctx)),
        "env" => Some(env_builtin(ctx, out)),
        "export" => Some(export(args, ctx, out)),
        "unset" => Some(unset(args, ctx)),
        "help" => Some(help(out)),
        "clear" => Some(clear(out)),
        "true" => Some(0),
        "false" => Some(1),
        // File builtins
        "ls" => Some(ls(args, ctx, out)),
        "cat" => Some(cat(args, ctx, out)),
        "mkdir" => Some(mkdir_builtin(args, ctx, out)),
        "touch" => Some(touch(args, ctx, out)),
        "rm" => Some(rm(args, ctx, out)),
        "cp" => Some(cp(args, ctx, out)),
        "mv" => Some(mv(args, ctx, out)),
        _ => None,
    }
}

// ── Non-file builtins ──────────────────────────────────────────────────────

fn echo(args: &[String], out: &mut String) -> i32 {
    let (no_newline, words) = if args.first().map(|s| s.as_str()) == Some("-n") {
        (true, &args[1..])
    } else {
        (false, args)
    };
    out.push_str(&words.join(" "));
    if !no_newline {
        out.push('\n');
    }
    0
}

fn pwd(ctx: &Context, out: &mut String) -> i32 {
    out.push_str(&ctx.env.cwd);
    out.push('\n');
    0
}

fn cd(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
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
        out.push_str(&format!("cd: {}: No such file or directory\n", new_cwd));
        return 1;
    }

    let old = ctx.env.cwd.clone();
    ctx.env.set("OLDPWD", &old);
    ctx.env.cwd = new_cwd.clone();
    ctx.env.set("PWD", &new_cwd);
    0
}

fn exit_builtin(args: &[String], ctx: &mut Context) -> i32 {
    let code = args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    ctx.env.last_exit_code = code;
    code
}

fn env_builtin(ctx: &Context, out: &mut String) -> i32 {
    let mut pairs: Vec<String> =
        ctx.env.vars().map(|(k, v)| format!("{}={}", k, v)).collect();
    pairs.sort();
    for pair in pairs {
        out.push_str(&pair);
        out.push('\n');
    }
    0
}

fn export(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    if args.is_empty() {
        return env_builtin(ctx, out);
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

fn help(out: &mut String) -> i32 {
    out.push_str(
        "wat — a small shell\n\
         \n\
         builtins: echo, pwd, cd, exit, env, export, unset, help, clear, true, false\n\
                   ls, cat, mkdir, touch, rm, cp, mv\n\
         \n\
         Hint: try `ls -a` to see what's around.\n",
    );
    0
}

fn clear(out: &mut String) -> i32 {
    out.push_str("\x1b[2J\x1b[H");
    0
}

// ── File builtins ──────────────────────────────────────────────────────────

fn ls(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
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
                        crate::vfs::FileType::Dir => "d",
                        crate::vfs::FileType::File => "-",
                    };
                    out.push_str(&format!("{} {}\n", kind, entry.name));
                } else {
                    out.push_str(&entry.name);
                    out.push('\n');
                }
            }
            0
        }
        Err(e) => {
            out.push_str(&format!("ls: {}\n", e));
            1
        }
    }
}

fn cat(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    if args.is_empty() {
        out.push_str("cat: no file specified\n");
        return 1;
    }
    let mut code = 0;
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        let path = resolve_path(arg, &ctx.env.cwd);
        match ctx.vfs.read(&path) {
            Ok(content) => {
                out.push_str(&String::from_utf8_lossy(&content));
            }
            Err(e) => {
                out.push_str(&format!("cat: {}\n", e));
                code = 1;
            }
        }
    }
    code
}

fn mkdir_builtin(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    if args.is_empty() {
        out.push_str("mkdir: missing operand\n");
        return 1;
    }
    let mut code = 0;
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        let path = resolve_path(arg, &ctx.env.cwd);
        if let Err(e) = ctx.vfs.mkdir(&path) {
            out.push_str(&format!("mkdir: {}\n", e));
            code = 1;
        }
    }
    code
}

fn touch(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    if args.is_empty() {
        out.push_str("touch: missing operand\n");
        return 1;
    }
    let mut code = 0;
    for arg in args {
        let path = resolve_path(arg, &ctx.env.cwd);
        // Create if not exists; no-op if already a file.
        if !ctx.vfs.exists(&path) {
            if let Err(e) = ctx.vfs.write(&path, b"") {
                out.push_str(&format!("touch: {}\n", e));
                code = 1;
            }
        }
    }
    code
}

fn rm(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    let recursive = args.iter().any(|a| {
        a == "-r" || a == "-rf" || a == "-fr" || a == "-R" || a == "-f" && false // -f alone isn't recursive
    }) || args.iter().any(|a| a.contains('r'));
    let force = args.iter().any(|a| a.contains('f'));

    let paths: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(|a| resolve_path(a, &ctx.env.cwd))
        .collect();

    if paths.is_empty() {
        out.push_str("rm: missing operand\n");
        return 1;
    }

    // Guard: never allow removing / or everything under /
    for path in &paths {
        if path == "/" || path == "/*" || path == "/~" {
            out.push_str(
                "rm: nice try. the void stares back, but your filesystem does not.\n",
            );
            return 1;
        }
    }

    let mut code = 0;
    for path in &paths {
        if let Err(e) = ctx.vfs.remove(path, recursive) {
            if !force {
                out.push_str(&format!("rm: {}\n", e));
                code = 1;
            }
        }
    }
    code
}

fn cp(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    let non_flags: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if non_flags.len() < 2 {
        out.push_str("cp: missing operand\n");
        return 1;
    }
    let src = resolve_path(non_flags[0], &ctx.env.cwd);
    let dst = resolve_path(non_flags[1], &ctx.env.cwd);
    match ctx.vfs.copy(&src, &dst) {
        Ok(()) => 0,
        Err(e) => {
            out.push_str(&format!("cp: {}\n", e));
            1
        }
    }
}

fn mv(args: &[String], ctx: &mut Context, out: &mut String) -> i32 {
    let non_flags: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if non_flags.len() < 2 {
        out.push_str("mv: missing operand\n");
        return 1;
    }
    let src = resolve_path(non_flags[0], &ctx.env.cwd);
    let dst = resolve_path(non_flags[1], &ctx.env.cwd);
    match ctx.vfs.rename(&src, &dst) {
        Ok(()) => 0,
        Err(e) => {
            out.push_str(&format!("mv: {}\n", e));
            1
        }
    }
}
