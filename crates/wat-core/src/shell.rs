use crate::env::Env;
use crate::eval::eval;
use crate::parser::parse;

pub struct Shell {
    pub env: Env,
    pub exit_requested: bool,
}

impl Shell {
    pub fn new() -> Self {
        Self { env: Env::new(), exit_requested: false }
    }

    pub fn prompt(&self) -> String {
        format!("5cotts@zo {} % ", self.env.prompt_cwd())
    }

    pub fn feed(&mut self, input: &str) -> String {
        let input = input.trim();
        if input.is_empty() {
            return String::new();
        }

        // Check for exit before parsing (handles `exit`, `exit N`)
        if input == "exit" {
            self.env.last_exit_code = 0;
            self.exit_requested = true;
            return String::new();
        }
        if let Some(rest) = input.strip_prefix("exit ") {
            let code = rest.trim().parse::<i32>().unwrap_or(0);
            self.env.last_exit_code = code;
            self.exit_requested = true;
            return String::new();
        }

        match parse(input) {
            Ok(list) => {
                let (_, output) = eval(&list, &mut self.env);
                output
            }
            Err(e) => format!("wat: {}\n", e),
        }
    }

    pub fn last_exit_code(&self) -> i32 {
        self.env.last_exit_code
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}
