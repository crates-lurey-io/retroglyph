# Packaging & Distribution Strategies for a Rust Terminal/Grid Library

A reference for distributing a Rust library (`retroglyph`) to crates.io, npm (WASM), and as prebuilt
binaries. Trimmed from an earlier, broader draft: the multi-language-bindings material (C FFI via
cbindgen, Python bindings via PyO3/maturin, pkg-config/system package integration) had no roadmap
support in any ADR and was cut. `retroglyph` targets Rust and WASM/npm consumers; if a C/Python/Ruby
binding story is ever pursued, it deserves its own decision and its own reference doc, not a
resurrection of this trimmed material (still recoverable from git history if needed).

---

## Table of Contents

1. [How BearLibTerminal Did It](#1-how-bearlibterminal-did-it)
2. [Repo Structure for Rust Core + FFI Bindings](#2-repo-structure)
3. [WASM/npm Packaging](#3-wasmnpm-packaging)
4. [Prebuilt Binary Distribution](#4-prebuilt-binary-distribution)
5. [Cross-Compilation Strategies](#5-cross-compilation-strategies)
6. [CI/CD for Multi-Platform Releases](#6-cicd-for-multi-platform-releases)

---

## 1. How BearLibTerminal Did It

BearLibTerminal used a "single shared library + language-specific header/wrapper" model:

- **Core**: One C++ dynamic library (`BearLibTerminal.dll` / `libBearLibTerminal.so` / `.dylib`)

  built with CMake, exposing a flat C API.

- **Bindings**: Thin header files or wrapper modules for C/C++, C#, Go, Lua, Pascal, Python, Ruby.

  Each wrapper called into the same shared library via its C API.

- **Distribution**: Platform-specific archives (`.zip` for Windows, `.tar.bz2` for Linux, `.zip` for

  macOS) containing 32-bit and 64-bit binaries, a showcase app, and all header files. Python also
  had a PyPI package (`pip install bearlibterminal`) bundling the native binary + Python wrapper.

- **Lua**: Built-in wrapper; the `.so`/`.dll` was loadable directly via `require "BearLibTerminal"`.
- **Key insight**: The flat C API (`terminal_open()`, `terminal_print()`, `terminal_read()`, etc.)

  was the universal interface. Every language binding was a thin FFI wrapper around these ~20
  functions.

**Takeaway for retroglyph**: Follow the same pattern. The Rust core library exposes a flat
`extern "C"` API. Language-specific bindings are thin wrappers. This is exactly what `cdylib` +
`cbindgen` gives you from Rust.

[Source: BearLibTerminal website](http://foo.wyrd.name/en:bearlibterminal) |
[Source: BearLibTerminal GitHub](https://github.com/cfyzium/bearlibterminal)

---

## 2. Repo Structure

Use a Cargo workspace with separate crates for core logic and each downstream binding target (WASM,
prebuilt binaries). This keeps concerns separated and allows independent versioning. `retroglyph`
already does this for its Rust-native surface (see the root `AGENTS.md` for the actual crate list);
the distribution-specific piece this section is about is the WASM/npm packaging layer:

```text
retroglyph/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── retroglyph-core/                  # Pure Rust library (lib crate)
│   │   ├── Cargo.toml            # crate-type = ["lib"]
│   │   └── src/lib.rs
│   └── retroglyph-wasm/                  # WASM bindings
│       ├── Cargo.toml            # crate-type = ["cdylib"], depends on wasm-bindgen
│       └── src/lib.rs            # #[wasm_bindgen] wrapping retroglyph-core
├── dist/                         # cargo-dist config, installer scripts
└── .github/workflows/
    ├── ci.yml                    # Test on all platforms
    └── release.yml               # Build + publish on tag
```

---

## 3. WASM/npm Packaging

### retroglyph-wasm/Cargo.toml

```toml
[package]
name = "retroglyph-wasm"
version.workspace = true
edition.workspace = true
description = "WASM bindings for the retroglyph terminal/grid library"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
retroglyph-core.workspace = true
wasm-bindgen = "0.2"
js-sys = "0.3"

[dev-dependencies]
wasm-bindgen-test = "0.3"

[profile.release]
opt-level = "s"       # Optimize for size in WASM
lto = true
```

### retroglyph-wasm/src/lib.rs

```rust
use wasm_bindgen::prelude::*;
use rg_core::{Terminal, Cell};

#[wasm_bindgen]
pub struct WasmTerminal {
    inner: Terminal,
}

#[wasm_bindgen]
impl WasmTerminal {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> Result<WasmTerminal, JsValue> {
        Terminal::new(width, height)
            .map(|t| WasmTerminal { inner: t })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn set(&mut self, x: u32, y: u32, ch: u32, fg: u32, bg: u32) {
        if let Some(c) = char::from_u32(ch) {
            self.inner.set(x, y, Cell { ch: c, fg, bg });
        }
    }

    pub fn print(&mut self, x: u32, y: u32, text: &str) {
        self.inner.print(x, y, text);
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 { self.inner.width() }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 { self.inner.height() }

    /// Return the grid as a flat Uint32Array: [ch, fg, bg, ch, fg, bg, ...]
    /// for efficient transfer to JS rendering.
    pub fn to_buffer(&self) -> Vec<u32> {
        self.inner.cells().iter().flat_map(|c| {
            [c.ch as u32, c.fg, c.bg]
        }).collect()
    }
}
```

### Building and Publishing (2)

```bash
# Install wasm-pack

cargo install wasm-pack

# Build for npm (browser target)

cd crates/retroglyph-wasm
wasm-pack build --target web --release

# Build for Node.js

wasm-pack build --target nodejs --release

# Build for bundlers (webpack, vite, etc.) - this is the default

wasm-pack build --release

# Pack into a tarball

wasm-pack pack

# Publish to npm

wasm-pack publish

# Publish with a tag

wasm-pack publish --tag next
```

The `pkg/` directory produced by `wasm-pack build` contains:

- `rg_wasm_bg.wasm` - the compiled WebAssembly binary
- `rg_wasm.js` - JavaScript glue code
- `rg_wasm.d.ts` - TypeScript type definitions (auto-generated)
- `package.json` - npm package metadata

### package.json customization

Add a `package.json` template in the crate root or edit the generated one:

```json
{
  "name": "@retroglyph/wasm",
  "version": "0.1.0",
  "description": "Terminal/grid library for roguelikes - WebAssembly build",
  "main": "rg_wasm.js",
  "types": "rg_wasm.d.ts",
  "files": ["rg_wasm_bg.wasm", "rg_wasm.js", "rg_wasm.d.ts"],
  "repository": {
    "type": "git",
    "url": "https://github.com/you/retroglyph"
  },
  "license": "MIT"
}
```

[Source: wasm-pack docs](https://rustwasm.github.io/docs/wasm-pack/) |
[Source: wasm-pack pack and publish](https://rustwasm.github.io/docs/wasm-pack/commands/pack-and-publish.html)

---

## 4. Prebuilt Binary Distribution

### cargo-dist (now called "dist")

cargo-dist auto-generates CI workflows that build platform-specific tarballs and installers on every
tagged release.

```bash
# Install

cargo install cargo-dist

# Initialize in your project (interactive)

cargo dist init

# Generate the CI workflow

cargo dist generate

# Plan a release (dry run)

cargo dist plan

# Build locally

cargo dist build
```

The generated `release.yml` workflow does the full pipeline on `git push --tags`:

1. **Plan**: Determine which crates to release based on the tag
2. **Build**: Spin up runners for each platform (Linux x86_64, macOS ARM64, Windows x86_64, etc.)
3. **Build artifacts**: tarballs, installers (shell script, PowerShell, Homebrew tap, MSI, etc.)
4. **Host**: Upload artifacts to GitHub Releases
5. **Announce**: Create/edit the GitHub Release with changelogs

### Cargo.toml additions for cargo-dist

```toml
# In workspace root Cargo.toml

[workspace.metadata.dist]
# CI backends

ci = "github"

# The installers to generate

installers = ["shell", "powershell", "homebrew"]

# Target platforms

targets = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu",
]

# For a library, distribute the shared library + headers

# instead of (or in addition to) binaries

include = [
    "include/retroglyph.h",
    "include/retroglyph.hpp",
    "LICENSE",
    "README.md",
]
```

### Manual GitHub Releases (without cargo-dist)

If cargo-dist does not fit your needs (e.g., you need to distribute shared libraries rather than
executables), use a custom workflow. See [Section 10](10-cicd-for-multi-platform-releases) for the
full CI config.

[Source: cargo-dist GitHub](https://github.com/axodotdev/cargo-dist)

---

## 5. Cross-Compilation Strategies

### cross-rs

[cross-rs](https://github.com/cross-rs/cross) provides Docker-based "zero setup" cross compilation.
It is a drop-in replacement for `cargo` that uses pre-built Docker images with the correct
toolchain, sysroot, and QEMU for testing.

```bash
# Install (2)

cargo install cross --git https://github.com/cross-rs/cross

# Build for ARM Linux

cross build --target aarch64-unknown-linux-gnu --release

# Test on emulated architecture (uses QEMU)

cross test --target aarch64-unknown-linux-gnu
```

Supports 50+ targets including Linux (glibc/musl), Windows (MinGW), Android, FreeBSD, and bare-metal
ARM. macOS and MSVC targets are not directly supported due to licensing constraints but are
available via [cross-toolchains](https://github.com/cross-rs/cross-toolchains).

### zig cc as a linker

Zig bundles libc headers and startup files for dozens of targets in a single 45 MB download, making
it an excellent cross-compilation linker for Rust. Unlike cross-rs, it does not require Docker.

```bash
# Install zig

brew install zig  # or download from ziglang.org

# Configure Rust to use zig as the C compiler/linker

# .cargo/config.toml

```

```toml
# .cargo/config.toml (2)

[target.aarch64-unknown-linux-gnu]
linker = "zig-cc-aarch64-linux-gnu"

[target.x86_64-unknown-linux-musl]
linker = "zig-cc-x86_64-linux-musl"
```

Create wrapper scripts (zig does not accept rustc's linker flag format directly):

```bash
#!/bin/sh
# zig-cc-aarch64-linux-gnu

zig cc -target aarch64-linux-gnu "$@"
```

Or use [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) which handles this
automatically:

```bash
cargo install cargo-zigbuild

# Build for Linux aarch64 from macOS

cargo zigbuild --target aarch64-unknown-linux-gnu --release

# Build for specific glibc version

cargo zigbuild --target aarch64-unknown-linux-gnu.2.17 --release
```

### Comparison

| Method              | Docker Required | macOS Targets            | Windows (MSVC) | glibc Targeting   | Setup Complexity |
| ------------------- | --------------- | ------------------------ | -------------- | ----------------- | ---------------- |
| cross-rs            | Yes             | No (via extra toolchain) | MinGW only     | Via image version | Low              |
| zig cc              | No              | No                       | MinGW only     | Specific version  | Medium           |
| cargo-zigbuild      | No              | No                       | MinGW only     | Specific version  | Low              |
| Native (Xcode/MSVC) | No              | Yes                      | Yes            | N/A               | High             |

[Source: cross-rs](https://github.com/cross-rs/cross) |
[Source: zig cc blog post](https://andrewkelley.me/post/zig-cc-powerful-drop-in-replacement-gcc-clang.html)

---

## 6. CI/CD for Multi-Platform Releases

### Complete GitHub Actions workflow

This workflow builds the shared library, static library, and headers for all major platforms, and
publishes Python wheels and npm packages.

```yaml
# .github/workflows/release.yml

name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always

jobs:
  # ─── Build WASM package ───
  build-wasm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust

        uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown

      - name: Install wasm-pack

        run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

      - name: Build WASM package

        run: |
          cd crates/retroglyph-wasm
          wasm-pack build --release --target web --scope retroglyph

      - uses: actions/upload-artifact@v4

        with:
          name: wasm-package
          path: crates/retroglyph-wasm/pkg/

  # ─── Publish everything ───
  publish:
    needs: [build-wasm]
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: actions/download-artifact@v4

        with:
          path: artifacts

      # Publish WASM to npm

      - name: Setup Node

        uses: actions/setup-node@v4
        with:
          node-version: '20'
          registry-url: 'https://registry.npmjs.org'

      - name: Publish to npm

        run: |
          cd artifacts/wasm-package
          npm publish --access public
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}

      # Publish Rust crates to crates.io (in dependency order; see RELEASING.md)

      - name: Publish to crates.io

        run: |
          cargo publish -p retroglyph-core --token ${{ secrets.CARGO_REGISTRY_TOKEN }}
          sleep 30  # Wait for crates.io to index
          # ... remaining crates, in dependency order
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

### CI Test Workflow

```yaml
# .github/workflows/ci.yml

name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace

  # Test WASM build
  test-wasm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

        with:
          targets: wasm32-unknown-unknown

      - run: cargo install wasm-pack
      - run: cd crates/retroglyph-wasm && wasm-pack test --headless --chrome

  # Lint
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

        with:
          components: clippy, rustfmt

      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
```

---

## Summary of Distribution Channels

| Channel         | Tool                   | Artifact                       | Consumer                 |
| --------------- | ---------------------- | ------------------------------ | ------------------------ |
| crates.io       | `cargo publish`        | Rust crate (`retroglyph-core`) | Rust developers          |
| GitHub Releases | cargo-dist / custom CI | prebuilt binaries per platform | Users of a compiled tool |
| npm             | wasm-pack              | WASM + JS glue + `.d.ts`       | Web/Node.js developers   |

## Sources

- **Kept**: [Rust Reference: Linkage](https://doc.rust-lang.org/reference/linkage.html) -

  authoritative source for crate-type semantics (`cdylib`/`staticlib`/`lib`)

- **Kept**: [wasm-pack docs](https://rustwasm.github.io/docs/wasm-pack/) - WASM packaging and npm

  publishing

- **Kept**: [cargo-dist](https://github.com/axodotdev/cargo-dist) - automated binary distribution
- **Kept**: [cross-rs](https://github.com/cross-rs/cross) - Docker-based cross compilation for 50+

  targets

- **Kept**:

  [zig cc blog post](https://andrewkelley.me/post/zig-cc-powerful-drop-in-replacement-gcc-clang.html) -
  deep dive on zig as a cross-compilation linker

- **Kept**: [BearLibTerminal](http://foo.wyrd.name/en:bearlibterminal) - the original inspiration
  for

  this doc's research; retroglyph does not follow its single-DLL multi-language-bindings approach
  (see the note at the top of this file)

- **Dropped**: nickel-org/rust-mustache - not relevant (no multi-language distribution)
- **Dropped**: aspect-build/rules_py, rules_js - Bazel-specific, not applicable
- **Dropped** (sources that only supported the trimmed C-FFI/PyO3/pkg-config sections):

  mozilla/cbindgen, the PyO3 user guide, maturin's distribution docs, metatensor -- recoverable from
  git history alongside the content they supported.

## Gaps

1. **Homebrew formula creation**: cargo-dist can auto-generate Homebrew taps for prebuilt binaries.

1. **Linux distro packaging** (deb, rpm, AUR): Not covered in depth. Tools like `cargo-deb` and

   `cargo-rpm` exist but may need customization.

1. **Version synchronization**: Keeping versions aligned across crates.io, npm, and GitHub Releases

   requires tooling; see `RELEASING.md` for retroglyph's own answer (release-plz, adopted
   post-0.1.0).
