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

    /// Construct a shell backed by MemoryVfs, regardless of compile features.
    /// Used in tests and the WASM target.
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

        match parse(input) {
            Ok(list) => {
                let (_, output) = eval(&list, &mut self.ctx);
                output
            }
            Err(e) => format!("wat: {}\n", e),
        }
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
