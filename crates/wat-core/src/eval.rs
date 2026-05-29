use crate::ast::{Command, List, Pipeline, Separator};
use crate::builtins::run_builtin;
use crate::env::Env;
use crate::expand::expand_word;

/// Evaluate a parsed command list. Returns `(exit_code, stdout_output)`.
pub fn eval(list: &List, env: &mut Env) -> (i32, String) {
    let mut out = String::new();
    let mut last_code = 0i32;

    let mut iter = list.0.iter().peekable();
    while let Some((pipeline, sep)) = iter.next() {
        let (code, pipeline_out) = eval_pipeline(pipeline, env);
        out.push_str(&pipeline_out);
        env.last_exit_code = code;
        last_code = code;

        match sep {
            Separator::And => {
                // `&&`: skip next pipeline on failure
                if code != 0 {
                    if let Some(_) = iter.peek() {
                        iter.next(); // skip
                    }
                }
            }
            Separator::Or => {
                // `||`: skip next pipeline on success
                if code == 0 {
                    if let Some(_) = iter.peek() {
                        iter.next(); // skip
                    }
                }
            }
            Separator::Semi | Separator::End => {}
        }
    }

    (last_code, out)
}

fn eval_pipeline(pipeline: &Pipeline, env: &mut Env) -> (i32, String) {
    // Phase 2: run each command independently; actual piping is Phase 4.
    let mut out = String::new();
    let mut last_code = 0;
    for cmd in &pipeline.0 {
        let (code, cmd_out) = eval_command(cmd, env);
        out.push_str(&cmd_out);
        last_code = code;
        env.last_exit_code = code;
    }
    (last_code, out)
}

fn eval_command(cmd: &Command, env: &mut Env) -> (i32, String) {
    let name = expand_word(&cmd.name, env);
    let args: Vec<String> = cmd.args.iter().map(|a| expand_word(a, env)).collect();

    let mut out = String::new();

    match run_builtin(&name, &args, env, &mut out) {
        Some(code) => (code, out),
        None => {
            // Unknown command
            out.push_str(&format!("wat: command not found: {}\n", name));
            (127, out)
        }
    }
}
