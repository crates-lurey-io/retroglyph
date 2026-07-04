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
    cargo clippy --workspace --all-targets --all-features -- -D warnings

lint: clippy markdown

# ── Build ────────────────────────────────────────────────────────────────────

compile:
    cargo check --workspace --all-features

doc:
    cargo doc --workspace --no-deps --document-private-items --all-features
    @./tools/gen-llms-txt.sh target/doc
    @cp -r docs/public/. target/doc/ 2>/dev/null || true
    @sed -i.bak "s/__GIT_SHA__/$(git rev-parse --short HEAD 2>/dev/null || echo unknown)/g" target/doc/index.html && rm -f target/doc/index.html.bak || true

# Regenerate the workspace-level llms.txt / llms-full.txt (root only); `just doc`
# also generates per-crate copies under target/doc/<crate>/.
llms:
    @./.bin/manual/bin/cargo-llms-txt 2>/dev/null || cargo llms-txt 2>/dev/null || true

docs-preview: doc
    @if command -v xdg-open > /dev/null; then xdg-open target/doc/index.html; \
    elif command -v open > /dev/null; then open target/doc/index.html; \
    fi

# ── Test ─────────────────────────────────────────────────────────────────────

test:
    # Build examples first so e2e_snapshot tests can find them.
    cargo build -p retroglyph-examples --examples --all-features
    cargo test --workspace --all-features

test-v:
    cargo build -p retroglyph-examples --examples --all-features
    cargo test --workspace --all-features -- --nocapture

# Run benchmarks locally. Install cargo-criterion first: cargo install cargo-criterion
# To save a baseline: just bench -- --save-baseline main
# To compare:        just bench -- --baseline main
bench *args:
    cargo bin cargo-criterion -p retroglyph-examples --bench retroglyph --features default-font {{ args }}

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
    cargo insta test --workspace --all-features && cargo insta accept

deny: deny-advisories deny-licenses

coverage:
    @which cargo-llvm-cov 2>/dev/null || cargo install cargo-llvm-cov
    cargo llvm-cov --workspace --lib --all-features --html --open

coverage-ci:
    @which cargo-llvm-cov 2>/dev/null || cargo install cargo-llvm-cov
    # --lib only: exclude integration tests. e2e_snapshots shells out to
    # `cargo build --example`, which lands in the default target dir, not
    # llvm-cov's separate --target-dir, so those binaries aren't found under
    # coverage. Lib unit tests are what we measure anyway.
    cargo llvm-cov --workspace --lib --all-features --lcov --output-path lcov.info

# ── Setup ────────────────────────────────────────────────────────────────────

setup-tools:
    cargo bin --install
    cargo install cargo-llms-txt --version 0.1.1 --root .bin/manual/ 2>/dev/null || true

setup-wasm: setup-tools

run-wasm:
    cargo run -p retroglyph-examples --target wasm32-unknown-unknown --example dungeon_room --features default-font

# ── act (local CI runner) ────────────────────────────────────────────────────

act-version := "v0.2.89"

act *args:
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="$PWD/.bin/manual"
    ACT="$BIN/act"
    if [ -f "$ACT" ]; then
        INSTALLED="$($ACT --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)"
        if [ "v$INSTALLED" = "{{act-version}}" ]; then
            exec "$ACT" -P ubuntu-latest=catthehacker/ubuntu:act-latest {{args}}
        fi
    fi
    echo "Installing act {{act-version}} to .bin/manual/..."
    mkdir -p "$BIN"
    OS="$(uname -s)"
    ARCH="$(uname -m | sed 's/aarch64/arm64/')"
    URL="https://github.com/nektos/act/releases/download/{{act-version}}/act_${OS}_${ARCH}.tar.gz"
    curl -sL "$URL" | tar xz -C "$BIN" act
    chmod +x "$ACT"
    echo "Installed act {{act-version}} to .bin/manual/act"
    exec "$ACT" -P ubuntu-latest=catthehacker/ubuntu:act-latest {{args}}
