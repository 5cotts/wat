use wasm_bindgen_test::*;
use wat_wasm::Shell;

wasm_bindgen_test_configure!(run_in_node_experimental);

#[wasm_bindgen_test]
fn shell_prompt_format() {
    let shell = Shell::new();
    assert_eq!(shell.prompt(), "5cotts@zo ~ % ");
}

#[wasm_bindgen_test]
fn shell_feed_echo_builtin() {
    let mut shell = Shell::new();
    let out = shell.feed("echo hello");
    assert_eq!(out, "hello\n");
}

#[wasm_bindgen_test]
fn shell_complete_returns_matches() {
    let shell = Shell::new();
    let completions = shell.complete("ec", 2);
    assert!(completions.contains(&"echo".to_string()));
}

#[wasm_bindgen_test]
fn shell_history_at_after_feed() {
    let mut shell = Shell::new();
    shell.feed("echo first");
    shell.feed("echo second");
    assert_eq!(shell.history_at(0), Some("echo second".to_string()));
    assert_eq!(shell.history_at(1), Some("echo first".to_string()));
}

#[wasm_bindgen_test]
fn wasm_external_command_falls_back_to_not_found() {
    // In the WASM bundle the default ProcessHost is NoopProcessHost, so any
    // command that isn't a builtin must surface a "command not found" error
    // — no spawn attempt, no panic.
    let mut shell = Shell::new();
    let out = shell.feed("git status");
    assert!(
        out.contains("command not found"),
        "expected command-not-found, got: {:?}",
        out
    );
}
