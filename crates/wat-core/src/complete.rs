use crate::vfs::Vfs;

const BUILTINS: &[&str] = &[
    "cat", "cd", "clear", "cp", "cut", "echo", "env", "exit", "export", "false", "grep", "head",
    "help", "ls", "mkdir", "mv", "pwd", "rm", "sort", "tail", "touch", "tr", "true", "uniq",
    "unset", "wc",
];

/// Return tab completions for `input[..cursor]`.
/// If completing the first token, match against builtins.
/// If completing a later token, match against files in `cwd`.
pub fn complete(input: &str, cursor: usize, cwd: &str, vfs: &dyn Vfs) -> Vec<String> {
    let partial = &input[..cursor.min(input.len())];

    // Tokenize roughly: split on whitespace
    let tokens: Vec<&str> = partial.split_whitespace().collect();
    let ends_with_space = partial.ends_with(' ') || partial.is_empty();

    if tokens.is_empty() || (tokens.len() == 1 && !ends_with_space) {
        // Completing the command name
        let prefix = tokens.first().copied().unwrap_or("");
        complete_command(prefix)
    } else {
        // Completing a file path argument
        let path_prefix = if ends_with_space {
            ""
        } else {
            tokens.last().copied().unwrap_or("")
        };
        complete_path(path_prefix, cwd, vfs)
    }
}

fn complete_command(prefix: &str) -> Vec<String> {
    BUILTINS
        .iter()
        .filter(|b| b.starts_with(prefix))
        .map(|s| s.to_string())
        .collect()
}

fn complete_path(prefix: &str, cwd: &str, vfs: &dyn Vfs) -> Vec<String> {
    // Split prefix into directory and filename stem
    let (dir, stem) = if let Some(pos) = prefix.rfind('/') {
        let d = if pos == 0 { "/" } else { &prefix[..pos] };
        let s = &prefix[pos + 1..];
        (d.to_string(), s.to_string())
    } else {
        (cwd.to_string(), prefix.to_string())
    };

    let entries = match vfs.list(&dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let dir_prefix = if dir == "/" {
        "/".to_string()
    } else if prefix.contains('/') {
        format!("{}/", dir)
    } else {
        String::new()
    };

    entries
        .iter()
        .filter(|e| e.name.starts_with(&stem))
        .map(|e| format!("{}{}", dir_prefix, e.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::MemoryVfs;

    fn vfs() -> MemoryVfs {
        let mut v = MemoryVfs::new();
        v.mkdir("/home").unwrap();
        v.mkdir("/home/user").unwrap();
        v.write("/home/user/file.txt", b"").unwrap();
        v.write("/home/user/foo.sh", b"").unwrap();
        v
    }

    #[test]
    fn complete_command_prefix() {
        let v = vfs();
        let completions = complete("ec", 2, "/home/user", &v);
        assert!(completions.contains(&"echo".to_string()));
    }

    #[test]
    fn complete_file_in_cwd() {
        let v = vfs();
        let completions = complete("cat f", 5, "/home/user", &v);
        assert!(completions.iter().any(|s| s.contains("file.txt")));
    }

    #[test]
    fn complete_absolute_path() {
        let v = vfs();
        let completions = complete("cat /home/user/f", 16, "/", &v);
        assert!(completions
            .iter()
            .any(|s| s.contains("file.txt") || s.contains("foo.sh")));
    }

    #[test]
    fn complete_empty_returns_all_builtins() {
        let v = vfs();
        let completions = complete("", 0, "/", &v);
        assert!(completions.len() >= BUILTINS.len());
    }

    #[test]
    fn complete_no_match_returns_empty() {
        let v = vfs();
        let completions = complete("cat /nonexistent/", 17, "/", &v);
        assert!(completions.is_empty());
    }

    #[test]
    fn complete_after_etc_m() {
        let mut v = MemoryVfs::new();
        v.mkdir("/etc").unwrap();
        v.write("/etc/motd", b"").unwrap();
        let completions = complete("cat /etc/m", 10, "/", &v);
        assert!(
            completions.contains(&"/etc/motd".to_string()),
            "got: {:?}",
            completions
        );
    }
}
