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
    # --exclude: neither is part of the published API surface (cargo-bin is a
    # dev tool, retroglyph-examples is unpublished demo/test code), so their
    # rustdoc has no business showing up on the docs site.
    cargo doc --workspace --no-deps --all-features --exclude retroglyph-examples --exclude cargo-bin
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

# Preview the full docs site locally, same content as what ships to GitHub
# Pages: rustdoc + llms.txt (via `doc`) plus the WASM examples gallery (via
# tools/build-wasm-examples.sh). Serves target/doc over real HTTP with
# miniserve (a `cargo bin`-managed local tool -- see workspace root
# Cargo.toml's [workspace.metadata.bin]), not file://, since the WASM demos
# fetch() their .wasm module and browsers block that from a file:// origin.
# Ctrl-C stops the server. Run `just setup-wasm` first if you haven't.
docs-preview-full: doc
    tools/build-wasm-examples.sh
    cargo bin --install
    @echo "Serving target/doc at http://localhost:8000 (Ctrl-C to stop)"
    @(sleep 1 && (command -v xdg-open > /dev/null && xdg-open http://localhost:8000 || open http://localhost:8000 2>/dev/null || true)) &
    cargo bin miniserve target/doc --port 8000 --index index.html -q

# ── Test ─────────────────────────────────────────────────────────────────────

# Builds every example with `--features crossterm` into its own `--target-dir` (see
# `examples/tests/support::build_crossterm_example`'s doc comment for why that dir is isolated
# from the workspace's normal `target/`), once, up front. Each of the 15 `svg_snapshot` tests
# (one `[[test]]` binary per example) calls `build_crossterm_example` itself too, as a fallback
# for running them outside `just` (e.g. `cargo nextest run -p retroglyph-examples` directly) --
# but under nextest those 15 tests run concurrently, and without this step they'd all race to
# invoke `cargo build` on the same target dir at once. Cargo's own locking makes that safe, but
# a dozen-plus processes queuing on one lock and each re-walking the dependency graph is real,
# avoidable overhead. Running the (single, batched) build here first means every one of those 15
# calls finds the binaries already fresh and returns immediately.
build-pty-examples:
    cargo build --manifest-path examples/Cargo.toml --examples --features crossterm --target-dir target/pty-examples

# nextest runs every test (including separate `[[test]]` binaries, like each
# examples/tests/*.rs file) in its own process, in parallel across all of them -- unlike plain
# `cargo test`, which runs separate integration-test binaries one after another. It doesn't run
# doctests (https://nexte.st/docs/limitations/), so those still go through plain `cargo test
# --doc`. See `.config/nextest.toml` for retry/timeout config.
test: build-pty-examples
    cargo bin cargo-nextest run --workspace --all-features
    cargo test --workspace --all-features --doc

# CI variant: assumes `nextest` is already on PATH as a prebuilt binary (e.g. installed via
# taiki-e/install-action) instead of being compiled from source through `cargo bin`, which is
# what made the CI `test` job take ~4 minutes longer than every other job.
#
# Uses the `ci` nextest profile (see `.config/nextest.toml`) instead of `default`: identical
# retry/timeout settings, but also writes JUnit XML to `target/nextest/ci/junit.xml`, which the
# `test` CI job uploads to Codecov's Test Analytics via `codecov/test-results-action`.
test-ci: build-pty-examples
    cargo nextest run --workspace --all-features --profile ci
    cargo test --workspace --all-features --doc

test-v: build-pty-examples
    cargo bin cargo-nextest run --workspace --all-features --no-capture
    cargo test --workspace --all-features --doc -- --nocapture

# Run every benchmark once, locally, no comparison. Args are forwarded to `cargo bench`/criterion:
#   just bench                                    # everything
#   just bench -- grid_diff/80x24                 # filter to one group
#   just bench -- grid_diff/80x24 --sample-size 20 # filter + fewer samples for a quick check
bench *args:
    cargo bench -p retroglyph-benches --benches {{ args }}

# Compare the current working copy (dirty changes included) against another git ref, default
# origin/main. See tools/bench-compare.sh for the full flag/example list (`-b <bench-name>`,
# forwarding extra criterion args after `--`, etc.):
#   just bench-compare                 # origin/main vs. current working copy
#   just bench-compare HEAD~5          # 5 commits back vs. current working copy
#   just bench-compare v0.3.0
#   just bench-compare -- grid_diff/80x24 --sample-size 20
bench-compare *args:
    ./tools/bench-compare.sh {{ args }}

# ── Dependencies ─────────────────────────────────────────────────────────────

deny-advisories:
    cargo deny check advisories

deny-licenses:
    cargo deny check bans licenses sources

# ── Composite ────────────────────────────────────────────────────────────────

# `compile` is deliberately not a dependency here: `lint` (clippy) already performs a full,
# strictly-stronger typecheck than plain `cargo check`, and `test` immediately after does a full
# build (also a superset of `check`). A standalone `cargo check --all-features` pass between them
# never catches anything those two don't already catch, and it's another full-workspace fingerprint
# pass for no extra correctness. `just compile` remains available on its own for a fast, cheap
# check-only iteration loop outside this composite.
check: fmt-check lint test doc

clean:
    cargo clean

# Prunes `target/` build artifacts untouched in the last 14 days, without a full `cargo clean`.
# Run this periodically (or wire into a cron/launchd job) to keep `target/` from accumulating
# stale incremental-compile variants across toolchain bumps and one-off feature combinations --
# `cargo clean` (above) is the nuclear option when you want a fully clean slate instead.
sweep:
    cargo bin cargo-sweep --time 14

# ── Convenience ──────────────────────────────────────────────────────────────

# Re-run every snapshot test and bless whatever changed. Deliberately plain `cargo test` plus the
# `insta` crate's own `INSTA_UPDATE` env var (already a dev-dependency everywhere snapshots live),
# not the separate `cargo-insta` CLI -- that would be a global tool this repo's tooling convention
# doesn't otherwise require (unlike `cargo bin`-managed tools in `[workspace.metadata.bin]`, or the
# `@which ... || cargo install ...` one-shot installs a couple of other recipes fall back to).
# Review the diff (`jj diff`/`git diff`) before committing -- this blesses unconditionally, with
# no review step of its own. Install `cargo-insta` by hand if you want its interactive review UI
# instead; nothing else in this repo depends on it being present.
insta: build-pty-examples
    INSTA_UPDATE=always cargo bin cargo-nextest run --workspace --all-features
    INSTA_UPDATE=always cargo test --workspace --all-features --doc

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
    # `cargo bin --install` (setup-tools) only builds/caches the binaries; it
    # never populates .bin/.shims (cargo-run-bin only syncs shims when a
    # binary is run via `cargo bin <name>`, not via `--install`). The wasm
    # target runner in .cargo/config.toml points at that shim, so force it
    # to be created here.
    cargo bin wasm-server-runner --version >/dev/null 2>&1 || true

run-wasm:
    cargo run -p retroglyph-examples --target wasm32-unknown-unknown --example 01_hello_world --features software

# Runs crates/terminal-wasm's tests/wasm_ffi.rs (the `wasm_terminal_*` FFI surface) under an
# actual wasm32 build via wasm-pack + Node.js -- the only place those `#[wasm_bindgen]`-exported
# functions run at all, since host-target `cargo test` never compiles that `cfg(target_arch =
# "wasm32")` module in the first place. `--node` (not `--chrome`/`--firefox`): this FFI has no
# DOM/xterm.js dependency to exercise (see that test file's doc comment), and Node avoids needing
# a browser + webdriver in CI. `wasm-pack test` sets its own cargo runner for the invocation, so
# it doesn't collide with the `wasm-server-runner` configured for `cfg(target_family = "wasm")` in
# .cargo/config.toml (that one's only for `just run-wasm`'s manual browser preview).
test-wasm:
    cargo bin wasm-pack test --node crates/terminal-wasm

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
