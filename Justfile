# Justfile for rg

default:
    @just --list

# ── Formatting ────────────────────────────────────────────────────────────────

rustfmt:
    cargo fmt --all -- --check

prettier:
    @[ -d tools/node_modules ] || npm ci --prefix tools
    npm --prefix tools run format:check

markdown:
    @[ -d tools/node_modules ] || npm ci --prefix tools
    npm --prefix tools run lint

fmt:
    cargo fmt --all
    @[ -d tools/node_modules ] || npm ci --prefix tools
    npm --prefix tools run format

# Local: check everything (rustfmt + prettier)
fmt-check: rustfmt prettier

# ── Linting ──────────────────────────────────────────────────────────────────

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

lint: clippy markdown

# ── Build ────────────────────────────────────────────────────────────────────

compile:
    cargo check --all-features

doc: _llms-txt
    cargo doc --no-deps --document-private-items --all-features
    @cp llms.txt llms-full.txt target/doc/ 2>/dev/null || true
    @cp -r docs/public/. target/doc/ 2>/dev/null || true
    @sed -i.bak "s/__GIT_SHA__/$(git rev-parse --short HEAD 2>/dev/null || echo unknown)/g" target/doc/index.html && rm -f target/doc/index.html.bak || true

llms: _llms-txt

_llms-txt:
    @./bin/bin/cargo-llms-txt 2>/dev/null || cargo llms-txt 2>/dev/null || true

docs-preview: doc
    @if command -v xdg-open > /dev/null; then xdg-open target/doc/index.html; \
    elif command -v open > /dev/null; then open target/doc/index.html; \
    fi

# ── Test ─────────────────────────────────────────────────────────────────────

test:
    # Build examples first so e2e_snapshot tests can find them.
    cargo build --examples --all-features
    cargo test --all-features

test-v:
    cargo build --examples --all-features
    cargo test --all-features -- --nocapture

# Run benchmarks locally. Install cargo-criterion first: cargo install cargo-criterion
# To save a baseline: just bench -- --save-baseline main
# To compare:        just bench -- --baseline main
bench *args:
    cargo bin cargo-criterion --bench retroglyph --features software-default-font {{ args }}

# ── Dependencies ─────────────────────────────────────────────────────────────

deny-advisories:
    cargo deny check advisories

deny-licenses:
    cargo deny check bans licenses sources

# ── Composite ────────────────────────────────────────────────────────────────

check: fmt-check lint compile test doc

clean:
    cargo clean

# ── Convenience ──────────────────────────────────────────────────────────────

insta:
    cargo insta test --all-features && cargo insta accept

deny: deny-advisories deny-licenses

coverage:
    @which cargo-llvm-cov 2>/dev/null || cargo install cargo-llvm-cov
    cargo llvm-cov --lib --all-features --html --open

coverage-ci:
    @which cargo-llvm-cov 2>/dev/null || cargo install cargo-llvm-cov
    cargo llvm-cov --lib --all-features --lcov --output-path lcov.info

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
