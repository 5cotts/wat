use crate::env::Env;

pub struct Shell {
    env: Env,
    pub exit_requested: bool,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            env: Env::new(),
            exit_requested: false,
        }
    }

    pub fn prompt(&self) -> String {
        format!("5cotts@zo {} % ", self.env.prompt_cwd())
    }

    pub fn feed(&mut self, input: &str) -> String {
        let input = input.trim();
        if input.is_empty() {
            return String::new();
        }

        if input == "exit" {
            self.env.last_exit_code = 0;
            self.exit_requested = true;
            return String::new();
        }
        if let Some(rest) = input.strip_prefix("exit ") {
            match rest.trim().parse::<i32>() {
                Ok(code) => {
                    self.env.last_exit_code = code;
                    self.exit_requested = true;
                    return String::new();
                }
                Err(_) => return "exit: invalid argument\n".to_string(),
            }
        }

        // Phase 0: echo input back
        format!("{}\n", input)
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
