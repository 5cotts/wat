use crate::env::Env;

/// Expand `$VAR`, `${VAR}`, `$?`, and leading `~` in a word.
/// Variable references expand to empty string if undefined.
pub fn expand_word(word: &str, env: &Env) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::with_capacity(word.len());
    let mut i = 0;

    // Leading ~ expands to $HOME (only when it's the whole token or followed by /)
    if !chars.is_empty() && chars[0] == '~' && (chars.len() == 1 || chars[1] == '/') {
        out.push_str(env.home());
        i = 1;
    }

    while i < chars.len() {
        if chars[i] == '$' {
            i += 1;
            if i >= chars.len() {
                out.push('$');
                break;
            }
            if chars[i] == '{' {
                // ${VAR}
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if i < chars.len() {
                    i += 1; // consume '}'
                }
                out.push_str(expand_var(&name, env));
            } else if chars[i] == '?' {
                out.push_str(&env.last_exit_code.to_string());
                i += 1;
            } else if chars[i].is_alphabetic() || chars[i] == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                out.push_str(expand_var(&name, env));
            } else {
                out.push('$');
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

fn expand_var<'a>(name: &str, env: &'a Env) -> &'a str {
    env.get(name).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;

    fn env() -> Env {
        let mut e = Env::new();
        e.set("FOO", "bar");
        e.set("HOME", "/home/5cotts");
        e
    }

    #[test]
    fn plain_word() {
        assert_eq!(expand_word("hello", &env()), "hello");
    }

    #[test]
    fn var_expansion() {
        assert_eq!(expand_word("$FOO", &env()), "bar");
    }

    #[test]
    fn braced_var() {
        assert_eq!(expand_word("${FOO}", &env()), "bar");
    }

    #[test]
    fn undefined_var_empty() {
        assert_eq!(expand_word("$UNDEF", &env()), "");
    }

    #[test]
    fn tilde_expands() {
        assert_eq!(expand_word("~", &env()), "/home/5cotts");
    }

    #[test]
    fn tilde_slash() {
        assert_eq!(expand_word("~/foo", &env()), "/home/5cotts/foo");
    }

    #[test]
    fn dollar_question() {
        let mut e = env();
        e.last_exit_code = 42;
        assert_eq!(expand_word("$?", &e), "42");
    }

    #[test]
    fn mixed_expansion() {
        assert_eq!(expand_word("${FOO}baz", &env()), "barbaz");
    }

    #[test]
    fn dollar_at_end() {
        assert_eq!(expand_word("end$", &env()), "end$");
    }
}
