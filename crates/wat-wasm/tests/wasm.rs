use wasm_bindgen_test::*;
use wat_wasm::Shell;

wasm_bindgen_test_configure!(run_in_node_experimental);

#[wasm_bindgen_test]
fn shell_prompt_format() {
    let shell = Shell::new();
    assert_eq!(shell.prompt(), "5cotts@zo ~ % ");
}

#[wasm_bindgen_test]
fn shell_feed_echoes() {
    let mut shell = Shell::new();
    let out = shell.feed("hello");
    assert_eq!(out, "hello\n");
}
