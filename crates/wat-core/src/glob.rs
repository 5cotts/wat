use crate::builtins::resolve::resolve_path;
use crate::vfs::Vfs;

/// Expand a glob pattern against the VFS. Returns a sorted list of matching paths.
/// If the pattern has no glob metacharacters, or no matches, returns `[pattern]`.
pub fn glob_expand(pattern: &str, vfs: &dyn Vfs, cwd: &str) -> Vec<String> {
    if !has_glob(pattern) {
        return vec![pattern.to_string()];
    }

    // Split on the first glob-containing segment, resolve the prefix directory.
    let (dir, file_pat) = split_glob_path(pattern, cwd);

    match vfs.list(&dir) {
        Err(_) => vec![pattern.to_string()],
        Ok(entries) => {
            let mut matches: Vec<String> = entries
                .iter()
                .filter(|e| match_glob(&file_pat, &e.name))
                .map(|e| {
                    if dir == "/" {
                        format!("/{}", e.name)
                    } else {
                        format!("{}/{}", dir, e.name)
                    }
                })
                .collect();
            if matches.is_empty() {
                vec![pattern.to_string()]
            } else {
                matches.sort();
                matches
            }
        }
    }
}

fn has_glob(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '*' | '?' | '['))
}

fn split_glob_path(pattern: &str, cwd: &str) -> (String, String) {
    // Find the last `/` before any glob metacharacter.
    let last_slash = pattern
        .char_indices()
        .take_while(|(_, c)| !matches!(c, '*' | '?' | '['))
        .filter(|(_, c)| *c == '/')
        .map(|(i, _)| i)
        .last();

    match last_slash {
        Some(i) => {
            let dir_part = &pattern[..i];
            let file_part = &pattern[i + 1..];
            let dir = resolve_path(if dir_part.is_empty() { "/" } else { dir_part }, cwd);
            (dir, file_part.to_string())
        }
        None => (cwd.to_string(), pattern.to_string()),
    }
}

/// Match a filename against a glob pattern (no `/` in either).
pub fn match_glob(pattern: &str, name: &str) -> bool {
    match_glob_chars(&pattern.chars().collect::<Vec<_>>(), &name.chars().collect::<Vec<_>>())
}

fn match_glob_chars(pat: &[char], name: &[char]) -> bool {
    match pat.first() {
        None => name.is_empty(),
        Some('*') => {
            // `*` matches zero or more characters (not `/`)
            for i in 0..=name.len() {
                if match_glob_chars(&pat[1..], &name[i..]) {
                    return true;
                }
            }
            false
        }
        Some('?') => {
            !name.is_empty() && match_glob_chars(&pat[1..], &name[1..])
        }
        Some('[') => {
            // Character class: [abc], [a-z], [!abc] (negated not implemented, skip)
            let (matched, rest) = match_char_class(&pat[1..], name.first().copied());
            matched && match_glob_chars(rest, if name.is_empty() { &[] } else { &name[1..] })
        }
        Some(p) => {
            name.first() == Some(p) && match_glob_chars(&pat[1..], &name[1..])
        }
    }
}

/// Returns (char_matched, remaining_pattern_after_]).
fn match_char_class(pat: &[char], c: Option<char>) -> (bool, &[char]) {
    let c = match c {
        Some(c) => c,
        None => return (false, pat),
    };

    let mut i = 0;
    let mut matched = false;
    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            if c >= pat[i] && c <= pat[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if pat[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    let rest = if i < pat.len() { &pat[i + 1..] } else { &pat[i..] };
    (matched, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_glob_returned_as_is() {
        use crate::vfs::MemoryVfs;
        let vfs = MemoryVfs::new();
        assert_eq!(glob_expand("hello", &vfs, "/"), vec!["hello"]);
    }

    #[test]
    fn star_matches_all() {
        assert!(match_glob("*", "anything"));
        assert!(match_glob("*", ""));
        assert!(match_glob("*.sh", "run.sh"));
        assert!(!match_glob("*.sh", "run.txt"));
    }

    #[test]
    fn question_matches_one() {
        assert!(match_glob("?", "a"));
        assert!(!match_glob("?", ""));
        assert!(!match_glob("?", "ab"));
        assert!(match_glob("a?c", "abc"));
    }

    #[test]
    fn char_class() {
        assert!(match_glob("[abc]", "a"));
        assert!(match_glob("[abc]", "b"));
        assert!(!match_glob("[abc]", "d"));
    }

    #[test]
    fn char_range() {
        assert!(match_glob("[a-z]", "m"));
        assert!(!match_glob("[a-z]", "M"));
        assert!(match_glob("[0-9]", "5"));
    }

    #[test]
    fn glob_expand_star_sh() {
        use crate::vfs::MemoryVfs;
        let mut vfs = MemoryVfs::new();
        vfs.mkdir("/home").unwrap();
        vfs.mkdir("/home/u").unwrap();
        vfs.write("/home/u/run.sh", b"").unwrap();
        vfs.write("/home/u/other.txt", b"").unwrap();
        let matches = glob_expand("*.sh", &vfs, "/home/u");
        assert_eq!(matches, vec!["/home/u/run.sh"]);
    }

    #[test]
    fn glob_expand_no_match_returns_pattern() {
        use crate::vfs::MemoryVfs;
        let vfs = MemoryVfs::new();
        let matches = glob_expand("*.sh", &vfs, "/");
        assert_eq!(matches, vec!["*.sh"]);
    }
}
