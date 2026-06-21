# Justfile for rg

default:
    @just --list

# ── Formatting ────────────────────────────────────────────────────────────────

rustfmt:
    cargo fmt --all -- --check

prettier:
    npm ci --prefix tools 2>/dev/null || true
    npm --prefix tools run format:check

markdown:
    npm --prefix tools run lint

fmt:
    cargo fmt --all
    npm --prefix tools run format

# Local: check everything (rustfmt + prettier)
fmt-check: rustfmt prettier

# ── Linting ──────────────────────────────────────────────────────────────────

clippy:
    cargo clippy --all-targets -- -D warnings

lint: clippy markdown

# ── Build ────────────────────────────────────────────────────────────────────

compile:
    cargo check --all-features

doc:
    cargo doc --no-deps --document-private-items
    @if [ -t 1 ]; then \
        cargo-llms-txt --output llms.txt --full llms-full.txt 2>/dev/null || true; \
        if command -v xdg-open > /dev/null; then xdg-open target/doc/retroglyph/index.html; \
        elif command -v open > /dev/null; then open target/doc/retroglyph/index.html; \
        fi \
    fi

# ── Test ─────────────────────────────────────────────────────────────────────

test:
    cargo test --all-features

test-v:
    cargo test --all-features -- --nocapture

# ── Dependencies ─────────────────────────────────────────────────────────────

deny-advisories:
    cargo deny check advisories

deny-licenses:
    cargo deny check bans licenses sources

# ── Generated files ──────────────────────────────────────────────────────────

llms:
    cargo-llms-txt --output llms.txt --full llms-full.txt

llms-check:
    cargo-llms-txt --output .llms-check.txt --full .llms-check-full.txt
    diff -q llms.txt .llms-check.txt && diff -q llms-full.txt .llms-check-full.txt
    rm -f .llms-check.txt .llms-check-full.txt

# ── Composite ────────────────────────────────────────────────────────────────

check: fmt-check lint compile test doc

clean:
    cargo clean

# ── Setup ────────────────────────────────────────────────────────────────────

setup-tools:
    cargo bin --install
    cargo install cargo-llms-txt --version 0.1.1 --root bin/ 2>/dev/null || true

setup-wasm:
    @if [ ! -f bin/bin/wasm-server-runner ]; then \
        echo "Installing wasm-server-runner 1.0.1 to bin/..."; \
        cargo binstall wasm-server-runner@1.0.1 --no-confirm --root bin/ 2>/dev/null || \
            cargo install wasm-server-runner --version 1.0.1 --locked --root bin/; \
    fi

run-wasm:
    cargo run --target wasm32-unknown-unknown --example wasm_demo --features software-default-font

# ── act (local CI runner) ────────────────────────────────────────────────────

act-version := "v0.2.89"

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
