use crate::env::Env;

/// Run a builtin command. Returns `Some(exit_code)` if the name is a known builtin,
/// `None` if the command is unknown (caller should treat as external command error).
pub fn run_builtin(name: &str, args: &[String], env: &mut Env, out: &mut String) -> Option<i32> {
    match name {
        "echo" => Some(echo(args, out)),
        "pwd" => Some(pwd(env, out)),
        "cd" => Some(cd(args, env, out)),
        "exit" => Some(exit_builtin(args, env)),
        "env" => Some(env_builtin(env, out)),
        "export" => Some(export(args, env, out)),
        "unset" => Some(unset(args, env)),
        "help" => Some(help(out)),
        "clear" => Some(clear(out)),
        "true" => Some(0),
        "false" => Some(1),
        _ => None,
    }
}

fn echo(args: &[String], out: &mut String) -> i32 {
    // Support `-n` flag (no trailing newline)
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

fn pwd(env: &Env, out: &mut String) -> i32 {
    out.push_str(&env.cwd);
    out.push('\n');
    0
}

fn cd(args: &[String], env: &mut Env, out: &mut String) -> i32 {
    let target = match args.first() {
        Some(p) => p.clone(),
        None => env.home().to_string(),
    };

    // Expand ~ manually (expand_word not called here to avoid circular dep)
    let target = if target == "~" {
        env.home().to_string()
    } else if target.starts_with("~/") {
        format!("{}{}", env.home(), &target[1..])
    } else if target == "-" {
        env.get("OLDPWD").unwrap_or("/").to_string()
    } else {
        target
    };

    let new_cwd = resolve_path(&target, &env.cwd);

    // Phase 3 will validate paths through the VFS; skip fs checks here.

    let old = env.cwd.clone();
    env.set("OLDPWD", &old);
    env.cwd = new_cwd.clone();
    env.set("PWD", &new_cwd);
    0
}

/// Resolve a path against a current working directory, handling `.` and `..`.
pub fn resolve_path(path: &str, cwd: &str) -> String {
    let base = if path.starts_with('/') {
        vec![]
    } else {
        cwd.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>()
    };

    let mut parts: Vec<&str> = base;
    for component in path.split('/').filter(|s| !s.is_empty()) {
        match component {
            "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }

    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn exit_builtin(args: &[String], env: &mut Env) -> i32 {
    let code = args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    env.last_exit_code = code;
    // Signals the shell to exit; the Shell wrapper checks exit_requested.
    code
}

fn env_builtin(env: &Env, out: &mut String) -> i32 {
    let mut pairs: Vec<String> = env.vars().map(|(k, v)| format!("{}={}", k, v)).collect();
    pairs.sort();
    for pair in pairs {
        out.push_str(&pair);
        out.push('\n');
    }
    0
}

fn export(args: &[String], env: &mut Env, out: &mut String) -> i32 {
    if args.is_empty() {
        return env_builtin(env, out);
    }
    for arg in args {
        if let Some((k, v)) = arg.split_once('=') {
            env.set(k, v);
        }
        // If no `=`, just mark as exported (we don't track export state separately yet)
    }
    0
}

fn unset(args: &[String], env: &mut Env) -> i32 {
    for arg in args {
        env.unset(arg);
    }
    0
}

fn help(out: &mut String) -> i32 {
    out.push_str(
        "wat — a small shell\n\
         \n\
         builtins: echo, pwd, cd, exit, env, export, unset, help, clear, true, false\n\
         \n\
         Hint: try `ls -a` to see what's around.\n",
    );
    0
}

fn clear(out: &mut String) -> i32 {
    // ANSI clear-screen + cursor home
    out.push_str("\x1b[2J\x1b[H");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    fn env() -> Env {
        Env::new()
    }

    #[test]
    fn echo_basic() {
        let mut out = String::new();
        let code = echo(&["hello".into(), "world".into()], &mut out);
        assert_eq!(code, 0);
        assert_eq!(out, "hello world\n");
    }

    #[test]
    fn echo_no_newline() {
        let mut out = String::new();
        echo(&["-n".into(), "hi".into()], &mut out);
        assert_eq!(out, "hi");
    }

    #[test]
    fn pwd_returns_cwd() {
        let mut e = env();
        e.cwd = "/tmp".to_string();
        let mut out = String::new();
        pwd(&e, &mut out);
        assert_eq!(out, "/tmp\n");
    }

    #[test]
    fn cd_updates_cwd_and_oldpwd() {
        let mut e = env();
        let mut out = String::new();
        let code = cd(&["/tmp".into()], &mut e, &mut out);
        assert_eq!(code, 0);
        assert_eq!(e.cwd, "/tmp");
        assert_eq!(e.get("OLDPWD"), Some("/home/5cotts"));
        assert_eq!(e.get("PWD"), Some("/tmp"));
    }

    #[test]
    fn cd_no_args_goes_home() {
        let mut e = env();
        e.cwd = "/tmp".to_string();
        let mut out = String::new();
        cd(&[], &mut e, &mut out);
        assert_eq!(e.cwd, "/home/5cotts");
    }

    #[test]
    fn cd_dotdot() {
        let mut e = env();
        e.cwd = "/a/b/c".to_string();
        let mut out = String::new();
        cd(&["..".into()], &mut e, &mut out);
        assert_eq!(e.cwd, "/a/b");
    }

    #[test]
    fn resolve_absolute() {
        assert_eq!(resolve_path("/foo/bar", "/home"), "/foo/bar");
    }

    #[test]
    fn resolve_relative() {
        assert_eq!(resolve_path("bar", "/foo"), "/foo/bar");
    }

    #[test]
    fn resolve_dotdot() {
        assert_eq!(resolve_path("../baz", "/foo/bar"), "/foo/baz");
    }

    #[test]
    fn resolve_to_root() {
        assert_eq!(resolve_path("../../..", "/a/b"), "/");
    }

    #[test]
    fn true_and_false() {
        let mut e = env();
        let mut out = String::new();
        assert_eq!(run_builtin("true", &[], &mut e, &mut out), Some(0));
        assert_eq!(run_builtin("false", &[], &mut e, &mut out), Some(1));
    }

    #[test]
    fn unknown_builtin_is_none() {
        let mut e = env();
        let mut out = String::new();
        assert_eq!(run_builtin("nonexistent", &[], &mut e, &mut out), None);
    }

    #[test]
    fn export_sets_var() {
        let mut e = env();
        let mut out = String::new();
        export(&["FOO=bar".into()], &mut e, &mut out);
        assert_eq!(e.get("FOO"), Some("bar"));
    }

    #[test]
    fn unset_removes_var() {
        let mut e = env();
        e.set("FOO", "bar");
        unset(&["FOO".into()], &mut e);
        assert_eq!(e.get("FOO"), None);
    }
}
