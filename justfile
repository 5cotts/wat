wasm-pack := ".cargo-tools/bin/wasm-pack"

# Install local tooling (run once after cloning)
bootstrap:
    cargo install wasm-pack --root .cargo-tools

# Build the native CLI binary
build-native:
    cargo build -p wat-cli --release

# Install wat to ~/.local/bin as a macOS scratch shell (see README — not a login shell)
install-mac: build-native
    mkdir -p ~/.local/bin
    cp target/release/wat ~/.local/bin/wat
    @echo "Installed wat to ~/.local/bin/wat"
    @echo "Make sure ~/.local/bin is on your PATH."

# Build the WASM package for the web (dev mode)
build-wasm:
    {{wasm-pack}} build crates/wat-wasm --target web --out-dir ../../web/pkg

# Build an optimized release WASM package
build-wasm-release:
    RUSTFLAGS="-C panic=abort" {{wasm-pack}} build crates/wat-wasm --release --target web --out-dir ../../web/pkg

# Start the Vite dev server (run build-wasm first)
dev-web: build-wasm
    cd web && bun install && bun run dev

# Run all tests (Rust unit + WASM node tests)
test:
    cargo test --workspace
    {{wasm-pack}} test --node crates/wat-wasm

# Full CI check
ci:
    cargo fmt --check
    cargo clippy --workspace -- -D warnings
    just test
    just build-wasm
    cd web && bun install && bun run build
