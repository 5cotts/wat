/// Resolve a path against the current working directory.
/// Handles absolute paths, `.`, `..`, and relative segments.
pub fn resolve_path(path: &str, cwd: &str) -> String {
    let base = if path.starts_with('/') {
        vec![]
    } else {
        cwd.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>()
    };

    let mut parts: Vec<&str> = base;
    for component in path.split('/').filter(|s| !s.is_empty()) {
        match component {
            "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }

    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute() {
        assert_eq!(resolve_path("/foo/bar", "/home"), "/foo/bar");
    }

    #[test]
    fn relative() {
        assert_eq!(resolve_path("bar", "/foo"), "/foo/bar");
    }

    #[test]
    fn dotdot() {
        assert_eq!(resolve_path("../baz", "/foo/bar"), "/foo/baz");
    }

    #[test]
    fn to_root() {
        assert_eq!(resolve_path("../../..", "/a/b"), "/");
    }

    #[test]
    fn dot_is_noop() {
        assert_eq!(resolve_path(".", "/foo"), "/foo");
    }
}
