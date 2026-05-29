use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Shell {
    inner: wat_core::Shell,
}

#[wasm_bindgen]
impl Shell {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Shell {
        Shell { inner: wat_core::Shell::with_memory_vfs() }
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Shell {

    pub fn prompt(&self) -> String {
        self.inner.prompt()
    }

    pub fn feed(&mut self, input: &str) -> String {
        self.inner.feed(input)
    }

    pub fn complete(&self, input: &str, cursor: usize) -> Vec<String> {
        self.inner.complete(input, cursor)
    }

    pub fn history_at(&self, index: usize) -> Option<String> {
        self.inner.history_at(index)
    }
}
