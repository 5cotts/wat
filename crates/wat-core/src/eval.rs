use crate::ast::{List, Pipeline, Separator};
use crate::builtins::run_builtin;
use crate::context::Context;
use crate::expand::expand_word;

/// Evaluate a parsed command list. Returns `(exit_code, stdout_output)`.
pub fn eval(list: &List, ctx: &mut Context) -> (i32, String) {
    let mut out = String::new();
    let mut last_code = 0i32;

    let mut iter = list.0.iter().peekable();
    while let Some((pipeline, sep)) = iter.next() {
        let (code, pipeline_out) = eval_pipeline(pipeline, ctx);
        out.push_str(&pipeline_out);
        ctx.env.last_exit_code = code;
        last_code = code;

        match sep {
            Separator::And => {
                if code != 0 {
                    if iter.peek().is_some() {
                        iter.next();
                    }
                }
            }
            Separator::Or => {
                if code == 0 {
                    if iter.peek().is_some() {
                        iter.next();
                    }
                }
            }
            Separator::Semi | Separator::End => {}
        }
    }

    (last_code, out)
}

fn eval_pipeline(pipeline: &Pipeline, ctx: &mut Context) -> (i32, String) {
    // Phase 2/3: run each command independently; piping wired in Phase 4.
    let mut out = String::new();
    let mut last_code = 0;
    for cmd in &pipeline.0 {
        let name = expand_word(&cmd.name, &ctx.env);
        let args: Vec<String> = cmd.args.iter().map(|a| expand_word(a, &ctx.env)).collect();
        let mut cmd_out = String::new();
        let code = match run_builtin(&name, &args, ctx, &mut cmd_out) {
            Some(c) => c,
            None => {
                cmd_out.push_str(&format!("wat: command not found: {}\n", name));
                127
            }
        };
        out.push_str(&cmd_out);
        last_code = code;
        ctx.env.last_exit_code = code;
    }
    (last_code, out)
}
