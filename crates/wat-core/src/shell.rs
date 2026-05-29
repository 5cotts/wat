use crate::complete::complete;
use crate::context::Context;
use crate::eval::eval;
use crate::parser::parse;

pub struct Shell {
    pub ctx: Context,
    pub exit_requested: bool,
}

impl Shell {
    pub fn new() -> Self {
        Self { ctx: Context::new(), exit_requested: false }
    }

    pub fn with_memory_vfs() -> Self {
        Self { ctx: Context::with_memory_vfs(), exit_requested: false }
    }

    pub fn prompt(&self) -> String {
        format!("5cotts@zo {} % ", self.ctx.env.prompt_cwd())
    }

    pub fn feed(&mut self, input: &str) -> String {
        let input = input.trim();
        if input.is_empty() {
            return String::new();
        }

        if input == "exit" {
            self.ctx.env.last_exit_code = 0;
            self.exit_requested = true;
            return String::new();
        }
        if let Some(rest) = input.strip_prefix("exit ") {
            let code = rest.trim().parse::<i32>().unwrap_or(0);
            self.ctx.env.last_exit_code = code;
            self.exit_requested = true;
            return String::new();
        }

        self.ctx.history.push(input);

        match parse(input) {
            Ok(list) => {
                let (_, output) = eval(&list, &mut self.ctx);
                output
            }
            Err(e) => format!("wat: {}\n", e),
        }
    }

    pub fn complete(&self, input: &str, cursor: usize) -> Vec<String> {
        complete(input, cursor, &self.ctx.env.cwd, self.ctx.vfs.as_ref())
    }

    pub fn history_at(&self, index: usize) -> Option<String> {
        self.ctx.history.at(index).map(|s| s.to_string())
    }

    pub fn last_exit_code(&self) -> i32 {
        self.ctx.env.last_exit_code
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}
