use std::collections::HashMap;

pub struct Env {
    pub vars: HashMap<String, String>,
    pub cwd: String,
    pub last_exit_code: i32,
}

impl Env {
    pub fn new() -> Self {
        let home = "/home/5cotts".to_string();
        let mut vars = HashMap::new();
        vars.insert("HOME".to_string(), home.clone());
        vars.insert("PWD".to_string(), home.clone());
        vars.insert(
            "PATH".to_string(),
            "/usr/local/bin:/usr/bin:/bin".to_string(),
        );
        Self {
            vars,
            cwd: home,
            last_exit_code: 0,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(|s| s.as_str())
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    pub fn unset(&mut self, key: &str) {
        self.vars.remove(key);
    }

    pub fn vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn home(&self) -> &str {
        self.vars
            .get("HOME")
            .map(|s| s.as_str())
            .unwrap_or("/home/5cotts")
    }

    pub fn prompt_cwd(&self) -> String {
        let home = self.home();
        if self.cwd == home {
            "~".to_string()
        } else if self.cwd.starts_with(home) {
            format!("~{}", &self.cwd[home.len()..])
        } else {
            self.cwd.clone()
        }
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}
