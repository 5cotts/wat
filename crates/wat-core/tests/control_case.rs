//! Tier 5 / Phase D: `case` / `esac`, glob-matched arms.

use wat_core::Shell;

fn shell() -> Shell {
    Shell::with_memory_vfs()
}

fn feed(sh: &mut Shell, input: &str) -> String {
    sh.feed(input)
}

#[test]
fn case_exact_match() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "case foo in foo) echo m;; *) echo no;; esac"),
        "m\n"
    );
}

#[test]
fn case_glob_pattern() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "case foo.txt in *.txt) echo t;; esac"), "t\n");
}

#[test]
fn case_alternation() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "case b in a|b|c) echo hit;; esac"), "hit\n");
    assert_eq!(
        feed(&mut sh, "case z in a|b|c) echo hit;; *) echo miss;; esac"),
        "miss\n"
    );
}

#[test]
fn case_catch_all() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "case anything in *) echo always;; esac"),
        "always\n"
    );
}

#[test]
fn case_no_match_is_noop_exit_zero() {
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "case foo in bar) echo x;; esac"), "");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "0");
}

#[test]
fn case_first_match_wins() {
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "case foo in foo) echo first;; foo) echo second;; esac"
        ),
        "first\n"
    );
    // A glob and the catch-all: the glob matches first.
    assert_eq!(
        feed(&mut sh, "case ab in a*) echo glob;; *) echo all;; esac"),
        "glob\n"
    );
}

#[test]
fn case_subject_is_expanded() {
    let mut sh = shell();
    feed(&mut sh, "x=foo");
    assert_eq!(feed(&mut sh, "case $x in foo) echo y;; esac"), "y\n");
}

#[test]
fn case_with_parenthesized_pattern() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "case foo in (foo) echo paren;; esac"),
        "paren\n"
    );
    assert_eq!(feed(&mut sh, "case b in (a|b) echo hit;; esac"), "hit\n");
}

#[test]
fn case_multi_command_body() {
    let mut sh = shell();
    assert_eq!(
        feed(&mut sh, "case x in x) echo a; echo b;; esac"),
        "a\nb\n"
    );
}

#[test]
fn case_body_exit_code() {
    let mut sh = shell();
    feed(&mut sh, "case x in x) false;; esac");
    assert_eq!(feed(&mut sh, "echo $?").trim(), "1");
}

#[test]
fn case_last_arm_without_double_semicolon() {
    // bash allows omitting `;;` on the final arm before `esac` (a separator is
    // still required so `esac` lands in command position).
    let mut sh = shell();
    assert_eq!(feed(&mut sh, "case x in x) echo ok\nesac"), "ok\n");
}

#[test]
fn case_inside_for() {
    let mut sh = shell();
    assert_eq!(
        feed(
            &mut sh,
            "for f in a.txt b.md c.txt; do case $f in *.txt) echo T:$f;; *) echo O:$f;; esac; done"
        ),
        "T:a.txt\nO:b.md\nT:c.txt\n"
    );
}
