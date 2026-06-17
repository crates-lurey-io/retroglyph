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

doc:
    cargo doc --no-deps --document-private-items

clean:
    cargo clean
