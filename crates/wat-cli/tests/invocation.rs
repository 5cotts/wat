//! Tier 6 / Phase B: non-interactive invocation — `wat -c`, `wat FILE`, and
//! a script piped on stdin. Drives the built binary via `std::process`.

use std::io::Write;
use std::process::{Command, Stdio};

fn wat() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wat"))
}

fn run_c(src: &str, extra: &[&str]) -> (String, i32) {
    let out = wat().arg("-c").arg(src).args(extra).output().expect("run");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn dash_c_runs_and_exits_zero() {
    let (out, code) = run_c("echo hi", &[]);
    assert_eq!(out, "hi\n");
    assert_eq!(code, 0);
}

#[test]
fn dash_c_propagates_exit_code() {
    let (_out, code) = run_c("exit 7", &[]);
    assert_eq!(code, 7);
}

#[test]
fn exit_stops_the_script() {
    let (out, code) = run_c("echo a; exit; echo b", &[]);
    assert_eq!(out, "a\n");
    assert_eq!(code, 0);
}

#[test]
fn dash_c_positional_params() {
    // `-c SRC name arg1 arg2` → $0=name, $1=arg1, ...
    let (out, _code) = run_c("echo $0 $1 $2 $#", &["myname", "foo", "bar"]);
    assert_eq!(out, "myname foo bar 2\n");
}

#[test]
fn dash_c_multiline_control_flow() {
    let (out, _code) = run_c("for x in a b c; do echo $x; done", &[]);
    assert_eq!(out, "a\nb\nc\n");
}

#[test]
fn script_file_with_args() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("wat_invoke_{}.sh", std::process::id()));
    std::fs::write(
        &path,
        "#!/usr/bin/env wat\necho \"script $0 got $1 and $2\"\nexit 3\n",
    )
    .expect("write script");

    let out = wat()
        .arg(&path)
        .arg("alpha")
        .arg("beta")
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("got alpha and beta"), "got: {:?}", stdout);
    assert_eq!(out.status.code(), Some(3));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn missing_script_file_errors() {
    let out = wat().arg("/no/such/wat/script").output().expect("run");
    assert_eq!(out.status.code(), Some(127));
}

#[test]
fn piped_stdin_is_run_as_script() {
    let mut child = wat()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"echo piped; echo two\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "piped\ntwo\n");
}

#[test]
fn dash_c_missing_argument() {
    let out = wat().arg("-c").output().expect("run");
    assert_eq!(out.status.code(), Some(2));
}
