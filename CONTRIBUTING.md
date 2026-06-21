# Contributing to retroglyph

## Development

Prerequisites:

- Rust (latest stable)
- Node.js (v22.12.0 LTS via `.nvmrc`)

### Build and Test

We use a local `Justfile` in the `tools/` directory to manage tooling.

```bash
# Run all formatters (cargo fmt + prettier)

just fmt

# Run linters (clippy + markdownlint)

just lint

# Run tests

just test
```
