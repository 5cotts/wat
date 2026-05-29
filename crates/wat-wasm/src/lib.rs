use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Shell {
    inner: wat_core::Shell,
}

#[wasm_bindgen]
impl Shell {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Shell {
        Shell {
            inner: wat_core::Shell::new(),
        }
    }

    pub fn prompt(&self) -> String {
        self.inner.prompt()
    }

    pub fn feed(&mut self, input: &str) -> String {
        self.inner.feed(input)
    }

    pub fn complete(&self, _input: &str, _cursor: usize) -> Vec<String> {
        vec![]
    }

    pub fn history_at(&self, _index: usize) -> Option<String> {
        None
    }
}
