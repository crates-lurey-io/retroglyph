# Justfile for rg

# Default target
default:
    @just --list

# --- Tools (Node/npm) ---
# Format markdown, JSON, YAML, JS, TS
fmt:
    cargo fmt --all
    npm --prefix tools run format

# Check formatting without changes
fmt-check:
    cargo fmt --all -- --check
    npm --prefix tools run format:check

# Run linters
lint:
    cargo clippy --all-targets -- -D warnings
    npm --prefix tools run lint

# --- Rust ---
test:
    cargo test --all-features

test-v:
    cargo test --all-features -- --nocapture

# Install cargo tools locally to bin/
setup-tools:
    cargo install cargo-llms-txt --root bin/

# Install wasm-server-runner (pinned) to local bin/ for running WASM examples in browser
setup-wasm:
    cargo install wasm-server-runner --version 1.0.1 --locked --root bin/

# Run the WASM demo in browser (requires `just setup-wasm` first)
run-wasm:
    cargo run --target wasm32-unknown-unknown --example wasm_demo --features software-default-font

# Generate llms.txt summary
llms:
    @if [ ! -f bin/bin/cargo-llms-txt ]; then \
        echo "Tool not found. Running setup-tools first..."; \
        just setup-tools; \
    fi
    bin/bin/cargo-llms-txt --output llms.txt --full llms-full.txt

# Check if llms.txt files are up-to-date
llms-check:
    @if [ ! -f bin/bin/cargo-llms-txt ]; then \
        echo "Tool not found. Running setup-tools first..."; \
        just setup-tools; \
    fi
    @mkdir -p .tmp_llms
    @bin/bin/cargo-llms-txt --output .tmp_llms/llms.txt --full .tmp_llms/llms-full.txt
    @if ! diff -q llms.txt .tmp_llms/llms.txt >/dev/null || ! diff -q llms-full.txt .tmp_llms/llms-full.txt >/dev/null; then \
        echo "Error: llms.txt files are out of date. Run 'just llms' to update."; \
        rm -rf .tmp_llms; \
        exit 1; \
    fi
    @rm -rf .tmp_llms
    @echo "llms.txt files are up-to-date."

# Generate rustdoc and open in browser if TTY
doc:
    cargo doc --no-deps --document-private-items
    @if [ -t 1 ]; then \
        if command -v xdg-open > /dev/null; then xdg-open target/doc/rg/index.html; \
        elif command -v open > /dev/null; then open target/doc/rg/index.html; \
        fi \
    fi

# Check everything
check: fmt-check lint test doc llms-check

clean:
    cargo clean
