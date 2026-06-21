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

# Install all project tools (pinned via [workspace.metadata.bin] in Cargo.toml)
setup-tools:
    cargo bin --install
    # cargo-llms-txt has no prebuilt binaries, so install to bin/ directly
    @if [ ! -f bin/bin/cargo-llms-txt ]; then \
        echo "Installing cargo-llms-txt 0.1.1 to bin/..."; \
        cargo install cargo-llms-txt --version 0.1.1 --root bin/; \
    fi

# Install wasm-server-runner (pinned) to local bin/ for WASM examples in browser.
# Needs a known path for .cargo/config.toml's runner, so kept separate from cargo-run-bin.
setup-wasm:
    @if [ ! -f bin/bin/wasm-server-runner ]; then \
        echo "Installing wasm-server-runner 1.0.1 to bin/..."; \
        cargo binstall wasm-server-runner@1.0.1 --no-confirm --root bin/ 2>/dev/null || \
            cargo install wasm-server-runner --version 1.0.1 --locked --root bin/; \
    fi

# Run the WASM demo in browser (requires `just setup-wasm` first)
run-wasm:
    cargo run --target wasm32-unknown-unknown --example wasm_demo --features software-default-font

# Generate llms.txt summary
llms:
    @LLMS="bin/bin/cargo-llms-txt"; \
    if [ ! -f "$$LLMS" ]; then \
        echo "Tool not found. Running setup-tools first..."; \
        just setup-tools; \
    fi; \
    "$$LLMS" --output llms.txt --full llms-full.txt

# Check if llms.txt files are up-to-date
llms-check:
    @LLMS="bin/bin/cargo-llms-txt"; \
    if [ ! -f "$$LLMS" ]; then \
        echo "Tool not found. Running setup-tools first..."; \
        just setup-tools; \
    fi; \
    mkdir -p .tmp_llms; \
    "$$LLMS" --output .tmp_llms/llms.txt --full .tmp_llms/llms-full.txt; \
    if ! diff -q llms.txt .tmp_llms/llms.txt >/dev/null || ! diff -q llms-full.txt .tmp_llms/llms-full.txt >/dev/null; then \
        echo "Error: llms.txt files are out of date. Run 'just llms' to update."; \
        rm -rf .tmp_llms; \
        exit 1; \
    fi; \
    rm -rf .tmp_llms; \
    echo "llms.txt files are up-to-date."

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

# Pinned version of act (not on crates.io, downloaded from GitHub releases)
act-version := "v0.2.89"

# Run GitHub Actions workflows locally via act
act *args:
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="$PWD/bin"
    ACT="$BIN/act"
    if [ -f "$ACT" ]; then
        INSTALLED="$($ACT --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)"
        if [ "v$INSTALLED" = "{{act-version}}" ]; then
            exec "$ACT" -P ubuntu-latest=catthehacker/ubuntu:act-latest {{args}}
        fi
    fi
    echo "Installing act {{act-version}} to bin/..."
    mkdir -p "$BIN"
    OS="$(uname -s)"
    ARCH="$(uname -m | sed 's/aarch64/arm64/')"
    URL="https://github.com/nektos/act/releases/download/{{act-version}}/act_${OS}_${ARCH}.tar.gz"
    curl -sL "$URL" | tar xz -C "$BIN" act
    chmod +x "$ACT"
    echo "Installed act {{act-version}} to bin/act"
    exec "$ACT" -P ubuntu-latest=catthehacker/ubuntu:act-latest {{args}}

clean:
    cargo clean
