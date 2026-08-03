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

prose:
    @command -v vale >/dev/null || { echo "vale not installed: brew install vale"; exit 1; }
    vale README.md CONTRIBUTING.md docs/ crates/

fmt:
    cargo fmt --all
    @[ -d tools/node_modules ] || npm ci --prefix tools
    npm --prefix tools run format

# Local: check everything (rustfmt + prettier)
fmt-check: rustfmt prettier

# ── Linting ──────────────────────────────────────────────────────────────────

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Typecheck the modules the host build skips (retroglyph#552).
#
# `retroglyph-gl` gates three test modules to other targets -- `headless.rs` to Linux,
# `webgl_smoke.rs` and `webgl_recovery.rs` to wasm32 -- so on any other host `just check` compiles
# none of them and can be green while they are broken. They are not incidental: all three drive
# the `Output` trait, so a change to the backend draw contract touches them. This is how #551 was
# locally green and failed five CI jobs.
#
# Deliberately not a dependency of `check`: it cold-compiles the workspace a second and third
# time, which would roughly triple the composite for a case most changes never hit. Run it when
# touching `Output`, `DrawCell`, or a backend implementation.
#
# wasm32 is scoped to `-p retroglyph-gl` because `crossterm` does not build for that target, which
# is also effectively what CI's `test-wasm-gl` job covers.
check-targets:
    cargo clippy --target x86_64-unknown-linux-gnu --workspace --all-targets --all-features -- -D warnings
    cargo clippy --target wasm32-unknown-unknown -p retroglyph-gl --all-targets --all-features -- -D warnings

lint: clippy markdown prose

# Checks every external URL in markdown and doc comments (lychee also parses links out of `.rs`
# files, so this covers doc comments too). Not part of `lint`/`check`: it hits the network, so
# it's scheduled-only in CI (.github/workflows/link-check.yml, retroglyph#469) rather than run on
# every push/PR. Uses the `cargo bin`-pinned `lychee` (see Cargo.toml's [workspace.metadata.bin])
# like every other CLI tool `cargo bin` manages in this repo, rather than a manually-installed
# `lychee` on PATH, so the version is reproducible and identical between local runs and CI.
link-check:
    cargo bin lychee --no-progress --exclude-path target --exclude-path .matan './**/*.md' './crates/**/*.rs'

# ── Features ─────────────────────────────────────────────────────────────────

# Regenerates each crate's Features doc section (in src/lib.rs and README.md) from the comments
# already sitting above its Cargo.toml [features] entries; see tools/gen-features. Also reflows
# the touched Markdown through prettier: prettier's proseWrap would otherwise immediately
# re-wrap the freshly generated prose differently on the next `just fmt`, and gen-features'
# own drift check is whitespace-insensitive specifically so the two tools converge instead of
# fighting over the same lines (see update_markers's doc comment in tools/gen-features).
gen-features:
    cargo gen-features
    @[ -d tools/node_modules ] || npm ci --prefix tools
    npm --prefix tools run format

# CI/local check: fails (with which files are stale) if any crate's Features doc section
# doesn't match its Cargo.toml. Folded into `doc` below rather than `lint`: it's fundamentally a
# docs-content check, and `doc` already walks every crate.
check-features:
    cargo gen-features --check

# ── Build ────────────────────────────────────────────────────────────────────

compile:
    cargo check --workspace --all-features
    # retroglyph#547: dep:gem is unconditional in retroglyph-core now, so this has to compile
    # with zero features, not just fewer -- the whole point of making it non-optional.
    cargo check -p retroglyph-core --no-default-features
    # retroglyph#882: retroglyph-widgets forwards a `std` feature to retroglyph-core's own, so
    # this is its `no_std` (alloc-only) build, the same reason retroglyph-core gets its own line
    # above.
    cargo check -p retroglyph-widgets --no-default-features

doc: check-features
    # --exclude: none of the three are part of the published API surface (cargo-bin and
    # gen-features are dev tools, retroglyph-examples is unpublished demo/test code), so their
    # rustdoc has no business showing up on the docs site.
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude retroglyph-examples --exclude cargo-bin --exclude gen-features
    ./tools/gen-llms-txt.sh target/doc
    @cp -r docs/public/. target/doc/ 2>/dev/null || true
    ./tools/gen-crates-index.sh target/doc
    @sed -i.bak "s/__GIT_SHA__/$(git rev-parse --short HEAD 2>/dev/null || echo unknown)/g" target/doc/index.html && rm -f target/doc/index.html.bak

# Build docs the way docs.rs does, so feature-gated items pick up their "Available on `feature`
# only" badges (requires nightly, since `doc_auto_cfg` is unstable). Verifies the docs.rs
# `[package.metadata.docs.rs] rustdoc-args = ["--cfg", "docsrs"]` setup locally instead of finding
# out on the next publish.
doc-docsrs:
    RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --all-features --no-deps --open

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
    just test-default-features

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
    just test-default-features

# Every downstream crate pins `retroglyph-core = { default-features = false }`, so a plain
# `cargo test -p <crate>` (as the README's quick start does, and as any fresh clone would run)
# builds with `egc` off -- the opposite of every `--all-features` run above (retroglyph#757).
# `--all-features`/`--workspace` never exercises that half, so CI could stay green while the
# plain command failed on a clean clone (as it did for the `panel` title test until this fix).
#
# Deliberately not `cargo test --workspace` (no flags): a `--workspace` build unifies feature
# resolution across every member compiled in the same command, and `examples/Cargo.toml` depends
# on `retroglyph-core` with its own full defaults (`egc` on), which would silently turn `egc` back
# on for every crate here too. Naming packages instead avoids that.
#
# `retroglyph-core` itself is a separate line rather than another `-p` on the command above:
# selecting it directly as a primary package (rather than only as a transitive dependency) makes
# cargo apply *its own* declared defaults (`egc` and `std` on) regardless of what its consumers
# pin, so exercising its `--no-default-features` (no `std`, no `egc`) build needs its own explicit
# command (retroglyph#843).
test-default-features:
    cargo test -p retroglyph-widgets -p retroglyph-terminal -p retroglyph-crossterm -p retroglyph-window -p retroglyph-gl
    cargo test -p retroglyph-core --no-default-features
    # retroglyph#882: same rationale as the `retroglyph-core` line above, now that
    # `retroglyph-widgets` has its own `std` feature forwarding to `retroglyph-core`'s.
    cargo test -p retroglyph-widgets --no-default-features

test-v: build-pty-examples
    cargo bin cargo-nextest run --workspace --all-features --no-capture
    cargo test --workspace --all-features --doc -- --nocapture

# Run every benchmark once, locally, no comparison. Args are forwarded to `cargo bench`/criterion:
#   just bench                                    # everything
#   just bench -- grid_diff/80x24                 # filter to one group
#   just bench -- grid_diff/80x24 --sample-size 20 # filter + fewer samples for a quick check
bench *args:
    cargo bench --workspace --all-features --benches {{ args }}

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

# Runs crates/gl's live WebGL2 render smoke test (src/webgl_smoke.rs) in a real headless browser --
# the only place the wasm32/WebGL2 draw path actually executes (the `compile-wasm-gl` CI job just
# build-checks it, which is how the glow 0.16 texImage3D atlas-upload bug shipped green). `--chrome`
# (not `--node`): WebGL2 needs a browser + a `<canvas>`. `--headless` for CI. CI runners have no
# GPU, so crates/gl/webdriver.json launches Chrome with `--enable-unsafe-swiftshader` for a
# software WebGL2 implementation. `default-font` so the test builds a renderer from the embedded
# atlas (same gate as the native `headless` render tests).
test-wasm-gl:
    cargo bin wasm-pack test --headless --chrome crates/gl --features default-font,tilesets

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
