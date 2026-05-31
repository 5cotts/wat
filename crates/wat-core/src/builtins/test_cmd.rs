//! The `test` / `[` builtin (Tier 5 Phase E).
//!
//! POSIX subset: string tests (`-z`, `-n`, `=`, `!=`, a bare non-empty
//! string), integer comparisons (`-eq -ne -lt -le -gt -ge`), file tests
//! (`-e -f -d`, via the VFS), and a single leading `!` negation. Exit code is
//! 0 (true), 1 (false), or 2 (usage error).

use crate::builtins::resolve::resolve_path;
use crate::context::Context;
use crate::io::ShellIo;

/// Entry point for both `test` and `[`. For `[`, the final argument must be `]`.
pub fn test_builtin(name: &str, args: &[String], ctx: &Context, io: &mut ShellIo) -> i32 {
    let mut strs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    if name == "[" {
        if strs.last() != Some(&"]") {
            io.write_err("wat: [: missing ']'\n");
            return 2;
        }
        strs.pop(); // drop the trailing `]`
    }
    match eval_test(&strs, ctx) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(msg) => {
            io.write_err(&format!("wat: {}: {}\n", name, msg));
            2
        }
    }
}

fn eval_test(args: &[&str], ctx: &Context) -> Result<bool, String> {
    // A single leading `!` negates the rest (except a lone `!`, which is just a
    // non-empty string and so is true).
    if args.len() > 1 && args[0] == "!" {
        return Ok(!eval_test(&args[1..], ctx)?);
    }
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => eval_unary(args[0], args[1], ctx),
        3 => eval_binary(args[0], args[1], args[2]),
        _ => Err("too many arguments".to_string()),
    }
}

fn eval_unary(op: &str, operand: &str, ctx: &Context) -> Result<bool, String> {
    match op {
        "-z" => Ok(operand.is_empty()),
        "-n" => Ok(!operand.is_empty()),
        "-e" | "-f" | "-d" => {
            let path = resolve_path(operand, &ctx.env.cwd);
            let exists = ctx.vfs.exists(&path);
            Ok(match op {
                "-e" => exists,
                "-d" => ctx.vfs.is_dir(&path),
                "-f" => exists && !ctx.vfs.is_dir(&path),
                _ => unreachable!(),
            })
        }
        _ => Err(format!("unary operator expected: {}", op)),
    }
}

fn eval_binary(a: &str, op: &str, b: &str) -> Result<bool, String> {
    match op {
        "=" => Ok(a == b),
        "!=" => Ok(a != b),
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let ai = parse_int(a)?;
            let bi = parse_int(b)?;
            Ok(match op {
                "-eq" => ai == bi,
                "-ne" => ai != bi,
                "-lt" => ai < bi,
                "-le" => ai <= bi,
                "-gt" => ai > bi,
                "-ge" => ai >= bi,
                _ => unreachable!(),
            })
        }
        _ => Err(format!("binary operator expected: {}", op)),
    }
}

fn parse_int(s: &str) -> Result<i64, String> {
    s.trim()
        .parse::<i64>()
        .map_err(|_| format!("integer expression expected: {}", s))
}
