# wat

A POSIX-flavored shell written in Rust that runs natively and compiles to WebAssembly for the browser.

## First-time setup

All tooling is scoped to this directory — nothing is installed globally.

```sh
# Install wasm-pack into .cargo-tools/ (run once)
just bootstrap

# Add the WASM target (handled automatically by rust-toolchain.toml on first cargo build)
```

## Build

```sh
just build-native   # builds ./target/release/wat
just build-wasm     # builds web/pkg/ via wasm-pack
```

## Run

```sh
cargo run -p wat-cli          # dev REPL
just dev-web                  # Vite dev server at http://localhost:5173
```

## Test

```sh
just test   # cargo test --workspace + wasm-pack test --node
```

## CI

```sh
just ci   # fmt + clippy + test + wasm build + web build
```

## Project layout

```
crates/
  wat-core/   # pure shell logic (lexer, parser, eval, builtins)
  wat-cli/    # native REPL binary
  wat-wasm/   # wasm-bindgen cdylib for the browser
web/          # Vite + xterm.js Zo Site
```

## Adding a builtin

1. Add a module in `crates/wat-core/src/builtins/`.
2. Register it in `crates/wat-core/src/builtins/mod.rs`.
3. Add integration tests in `crates/wat-core/tests/builtins.rs`.

## Adding an easter egg

Implement it as a builtin in `wat-core` (see the builtins guide above). For side effects that require browser APIs (redirect, animation), emit an OSC `9999` escape sequence from `io::emit_side_effect()` and handle the payload type in `web/src/shell-bridge.ts`.
