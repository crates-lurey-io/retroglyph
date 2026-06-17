# Packaging & Distribution Strategies for a Rust Terminal/Grid Library

A comprehensive reference for distributing a Rust library (`rg`) with multi-language bindings,
modeled after the approach BearLibTerminal pioneered: a single native library with thin wrappers for
C, Python, Ruby, WASM, and more.

---

## Table of Contents

1. [How BearLibTerminal Did It](#1-how-bearlibterminal-did-it)
2. [Repo Structure for Rust Core + FFI Bindings](#2-repo-structure)
3. [cdylib vs staticlib Targets](#3-cdylib-vs-staticlib)
4. [C FFI from Rust (cbindgen)](#4-c-ffi-from-rust)
5. [Python Bindings via PyO3/maturin](#5-python-bindings)
6. [WASM/npm Packaging](#6-wasmnpm-packaging)
7. [Prebuilt Binary Distribution](#7-prebuilt-binary-distribution)
8. [pkg-config and System Package Integration](#8-pkg-config-integration)
9. [Cross-Compilation Strategies](#9-cross-compilation)
10. [CI/CD for Multi-Platform Releases](#10-cicd)

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

**Takeaway for rg**: Follow the same pattern. The Rust core library exposes a flat `extern "C"` API.
Language-specific bindings are thin wrappers. This is exactly what `cdylib` + `cbindgen` gives you
from Rust.

[Source: BearLibTerminal website](http://foo.wyrd.name/en:bearlibterminal) |
[Source: BearLibTerminal GitHub](https://github.com/cfyzium/bearlibterminal)

---

## 2. Repo Structure

Use a Cargo workspace with separate crates for core logic, C FFI, Python bindings, and WASM
bindings. This keeps concerns separated and allows independent versioning.

```
rg/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── rg-core/                  # Pure Rust library (lib crate)
│   │   ├── Cargo.toml            # crate-type = ["lib"]
│   │   └── src/lib.rs
│   ├── rg-ffi/                   # C FFI layer (cdylib + staticlib)
│   │   ├── Cargo.toml            # crate-type = ["cdylib", "staticlib"]
│   │   ├── cbindgen.toml
│   │   ├── build.rs              # Runs cbindgen to generate rg.h
│   │   └── src/lib.rs            # #[no_mangle] extern "C" functions
│   ├── rg-python/                # PyO3 Python bindings
│   │   ├── Cargo.toml            # crate-type = ["cdylib"], depends on pyo3
│   │   ├── pyproject.toml
│   │   └── src/lib.rs            # #[pymodule] wrapping rg-core
│   └── rg-wasm/                  # WASM bindings
│       ├── Cargo.toml            # crate-type = ["cdylib"], depends on wasm-bindgen
│       └── src/lib.rs            # #[wasm_bindgen] wrapping rg-core
├── include/                      # Generated headers (committed or CI-generated)
│   └── rg.h
├── bindings/
│   ├── python/                   # Pure Python wrapper (if needed beyond PyO3)
│   ├── lua/                      # Lua FFI wrapper
│   └── ruby/                     # Ruby FFI wrapper
├── dist/                         # cargo-dist config, installer scripts
└── .github/workflows/
    ├── ci.yml                    # Test on all platforms
    └── release.yml               # Build + publish on tag
```

### Root Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/rg-core",
    "crates/rg-ffi",
    "crates/rg-python",
    "crates/rg-wasm",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
repository = "https://github.com/you/rg"

[workspace.dependencies]
rg-core = { path = "crates/rg-core" }
```

---

## 3. cdylib vs staticlib

From the [Rust Reference on Linkage](https://doc.rust-lang.org/reference/linkage.html):

| Crate Type  | Output                       | Use Case                                       |
| ----------- | ---------------------------- | ---------------------------------------------- |
| `lib`       | Compiler-chosen Rust lib     | Normal Rust dependency                         |
| `rlib`      | Static Rust library          | Intermediate artifact for Rust consumers       |
| `dylib`     | Dynamic Rust library         | Rust-to-Rust dynamic linking (Rust ABI)        |
| `cdylib`    | Dynamic system library       | Loading from other languages (C, Python, etc.) |
| `staticlib` | Static system library (`.a`) | Linking into non-Rust applications             |

**For multi-language bindings, use both `cdylib` and `staticlib`:**

```toml
# crates/rg-ffi/Cargo.toml
[lib]
name = "rg"
crate-type = ["cdylib", "staticlib"]
```

- **`cdylib`** produces `.so` / `.dylib` / `.dll` with no Rust-specific metadata. This is what
  Python's ctypes, Ruby's FFI, Lua's `require`, and any C/C++ program will load. It strips unused
  Rust standard library code and does not export Rust internal symbols.
- **`staticlib`** produces `.a` / `.lib` containing all Rust code and upstream dependencies baked
  in. Used when someone wants to statically link rg into their C/C++ application. Note: any dynamic
  system dependencies (OpenGL, etc.) must be specified manually when linking.
- **You can specify both** in the same crate. Cargo will produce both artifacts in a single build.

The `rg-python` and `rg-wasm` crates each need only `cdylib` since PyO3 and wasm-bindgen both
produce dynamic libraries (`.so` for Python extension modules, `.wasm` for WebAssembly).

---

## 4. C FFI from Rust (cbindgen)

### The Pattern

1. Write `#[no_mangle] pub extern "C" fn` functions in the FFI crate.
2. Use `#[repr(C)]` on any structs that cross the FFI boundary.
3. Use `cbindgen` to auto-generate `rg.h` from the Rust source.

### rg-ffi/src/lib.rs

```rust
use rg_core::{Terminal, Cell};
use std::ffi::CStr;
use std::os::raw::c_char;

/// Opaque handle to a terminal instance.
/// Prevents C callers from touching Rust internals.
pub struct RgTerminal(Terminal);

#[repr(C)]
pub struct RgCell {
    pub ch: u32,       // Unicode codepoint
    pub fg: u32,       // RGBA foreground
    pub bg: u32,       // RGBA background
}

/// Create a new terminal with the given dimensions.
/// Returns null on failure. Caller must free with rg_terminal_destroy().
#[no_mangle]
pub extern "C" fn rg_terminal_create(width: u32, height: u32) -> *mut RgTerminal {
    match Terminal::new(width, height) {
        Ok(t) => Box::into_raw(Box::new(RgTerminal(t))),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy a terminal instance.
/// # Safety
/// `term` must be a valid pointer returned by rg_terminal_create(),
/// and must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn rg_terminal_destroy(term: *mut RgTerminal) {
    if !term.is_null() {
        drop(Box::from_raw(term));
    }
}

/// Set a cell at (x, y).
#[no_mangle]
pub unsafe extern "C" fn rg_terminal_set(
    term: *mut RgTerminal,
    x: u32,
    y: u32,
    cell: RgCell,
) {
    if let Some(t) = term.as_mut() {
        t.0.set(x, y, Cell {
            ch: char::from_u32(cell.ch).unwrap_or(' '),
            fg: cell.fg,
            bg: cell.bg,
        });
    }
}

/// Print a string at (x, y). The string must be valid UTF-8, null-terminated.
#[no_mangle]
pub unsafe extern "C" fn rg_terminal_print(
    term: *mut RgTerminal,
    x: u32,
    y: u32,
    text: *const c_char,
) {
    if term.is_null() || text.is_null() { return; }
    let t = &mut (*term).0;
    let s = CStr::from_ptr(text).to_str().unwrap_or("");
    t.print(x, y, s);
}
```

### cbindgen.toml

```toml
language = "C"
header = "/* Generated by cbindgen - do not edit */"
include_guard = "RG_H"
autogen_warning = "/* Warning: this file is autogenously generated by cbindgen. */"
tab_width = 4
style = "Both"        # Generate both C and C++ compatible output
cpp_compat = true

[defines]
# Map Rust cfg to C preprocessor defines if needed

[export]
prefix = "Rg"
include = []           # Empty = export everything marked pub extern "C"
exclude = []

[export.rename]
# Optionally rename types for C consumers

[fn]
# Function-level settings
args = "Vertical"

[struct]
rename_fields = "None"

[enum]
rename_variants = "ScreamingSnakeCase"
prefix_with_name = true
```

### build.rs (runs cbindgen at build time)

```rust
fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    let config = cbindgen::Config::from_file("cbindgen.toml")
        .expect("Failed to read cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("Failed to generate C bindings")
        .write_to_file("../../include/rg.h");

    // Also generate a C++ header
    let cpp_config = {
        let mut c = config;
        c.language = cbindgen::Language::Cxx;
        c
    };

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(cpp_config)
        .generate()
        .expect("Failed to generate C++ bindings")
        .write_to_file("../../include/rg.hpp");
}
```

### rg-ffi/Cargo.toml

```toml
[package]
name = "rg-ffi"
version.workspace = true
edition.workspace = true

[lib]
name = "rg"
crate-type = ["cdylib", "staticlib"]

[dependencies]
rg-core.workspace = true

[build-dependencies]
cbindgen = "0.28"
```

### Generated rg.h (example output)

```c
/* Generated by cbindgen - do not edit */

#ifndef RG_H
#define RG_H

#include <stdint.h>
#include <stdbool.h>

typedef struct RgTerminal RgTerminal;

typedef struct RgCell {
    uint32_t ch;
    uint32_t fg;
    uint32_t bg;
} RgCell;

RgTerminal *rg_terminal_create(uint32_t width, uint32_t height);

void rg_terminal_destroy(RgTerminal *term);

void rg_terminal_set(RgTerminal *term, uint32_t x, uint32_t y, struct RgCell cell);

void rg_terminal_print(RgTerminal *term, uint32_t x, uint32_t y, const char *text);

#endif /* RG_H */
```

[Source: mozilla/cbindgen](https://github.com/mozilla/cbindgen) |
[Source: metatensor (real-world cbindgen user)](https://github.com/metatensor/metatensor)

---

## 5. Python Bindings via PyO3/maturin

Two approaches exist; use both for different audiences:

### Approach A: PyO3 (native Python module)

Gives Pythonic API with type hints, proper exceptions, and zero-copy where possible.

#### rg-python/Cargo.toml

```toml
[package]
name = "rg-python"
version.workspace = true
edition.workspace = true

[lib]
name = "rg"
crate-type = ["cdylib"]

[dependencies]
rg-core.workspace = true
pyo3 = { version = "0.23", features = ["extension-module"] }
```

#### rg-python/pyproject.toml

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "rg-terminal"
requires-python = ">=3.8"
description = "A terminal/grid library for roguelikes and TUI applications"
classifiers = [
    "Programming Language :: Rust",
    "Programming Language :: Python :: Implementation :: CPython",
    "Programming Language :: Python :: Implementation :: PyPy",
    "License :: OSI Approved :: MIT License",
]

[tool.maturin]
# Build for the crate in the current directory
features = ["pyo3/extension-module"]
```

#### rg-python/src/lib.rs

```rust
use pyo3::prelude::*;
use rg_core::{Terminal, Cell};

#[pyclass]
struct PyTerminal {
    inner: Terminal,
}

#[pymethods]
impl PyTerminal {
    #[new]
    fn new(width: u32, height: u32) -> PyResult<Self> {
        Terminal::new(width, height)
            .map(|t| PyTerminal { inner: t })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    }

    fn set(&mut self, x: u32, y: u32, ch: char, fg: u32, bg: u32) {
        self.inner.set(x, y, Cell { ch, fg, bg });
    }

    fn print(&mut self, x: u32, y: u32, text: &str) {
        self.inner.print(x, y, text);
    }

    fn get(&self, x: u32, y: u32) -> Option<(char, u32, u32)> {
        self.inner.get(x, y).map(|c| (c.ch, c.fg, c.bg))
    }

    #[getter]
    fn width(&self) -> u32 { self.inner.width() }

    #[getter]
    fn height(&self) -> u32 { self.inner.height() }
}

#[pymodule]
fn rg(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyTerminal>()?;
    Ok(())
}
```

### Building and Publishing

```bash
# Development (installs into current virtualenv)
cd crates/rg-python
maturin develop

# Build wheels for current platform
maturin build --release

# Build manylinux wheels (for PyPI)
docker run --rm -v $(pwd):/io ghcr.io/pyo3/maturin build --release --manylinux 2014

# Publish to PyPI
maturin publish

# Generate CI workflow
maturin generate-ci github > ../../.github/workflows/python-release.yml
```

### Approach B: ctypes/cffi wrapper over the C FFI

For users who prefer a pure-Python wrapper that loads the prebuilt `.so`/`.dll`:

```python
# bindings/python/rg/__init__.py
import ctypes
import os
import platform

def _load_library():
    """Load the rg shared library."""
    system = platform.system()
    if system == "Windows":
        name = "rg.dll"
    elif system == "Darwin":
        name = "librg.dylib"
    else:
        name = "librg.so"

    # Look in the package directory first
    pkg_dir = os.path.dirname(os.path.abspath(__file__))
    lib_path = os.path.join(pkg_dir, name)
    if os.path.exists(lib_path):
        return ctypes.CDLL(lib_path)

    # Fall back to system search
    return ctypes.CDLL(name)

_lib = _load_library()

# Define function signatures
_lib.rg_terminal_create.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
_lib.rg_terminal_create.restype = ctypes.c_void_p

_lib.rg_terminal_destroy.argtypes = [ctypes.c_void_p]
_lib.rg_terminal_destroy.restype = None

class Terminal:
    def __init__(self, width: int, height: int):
        self._ptr = _lib.rg_terminal_create(width, height)
        if not self._ptr:
            raise RuntimeError("Failed to create terminal")

    def __del__(self):
        if hasattr(self, '_ptr') and self._ptr:
            _lib.rg_terminal_destroy(self._ptr)

    def print(self, x: int, y: int, text: str):
        _lib.rg_terminal_print(self._ptr, x, y, text.encode('utf-8'))
```

[Source: PyO3 getting started](https://pyo3.rs/v0.22.0/getting-started) |
[Source: maturin distribution guide](https://maturin.rs/distribution)

---

## 6. WASM/npm Packaging

### rg-wasm/Cargo.toml

```toml
[package]
name = "rg-wasm"
version.workspace = true
edition.workspace = true
description = "WASM bindings for the rg terminal/grid library"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
rg-core.workspace = true
wasm-bindgen = "0.2"
js-sys = "0.3"

[dev-dependencies]
wasm-bindgen-test = "0.3"

[profile.release]
opt-level = "s"       # Optimize for size in WASM
lto = true
```

### rg-wasm/src/lib.rs

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

### Building and Publishing

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for npm (browser target)
cd crates/rg-wasm
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
  "name": "@rg/wasm",
  "version": "0.1.0",
  "description": "Terminal/grid library for roguelikes - WebAssembly build",
  "main": "rg_wasm.js",
  "types": "rg_wasm.d.ts",
  "files": ["rg_wasm_bg.wasm", "rg_wasm.js", "rg_wasm.d.ts"],
  "repository": {
    "type": "git",
    "url": "https://github.com/you/rg"
  },
  "license": "MIT"
}
```

[Source: wasm-pack docs](https://rustwasm.github.io/docs/wasm-pack/) |
[Source: wasm-pack pack and publish](https://rustwasm.github.io/docs/wasm-pack/commands/pack-and-publish.html)

---

## 7. Prebuilt Binary Distribution

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
    "include/rg.h",
    "include/rg.hpp",
    "LICENSE",
    "README.md",
]
```

### Manual GitHub Releases (without cargo-dist)

If cargo-dist does not fit your needs (e.g., you need to distribute shared libraries rather than
executables), use a custom workflow. See [Section 10](#10-cicd) for the full CI config.

[Source: cargo-dist GitHub](https://github.com/axodotdev/cargo-dist)

---

## 8. pkg-config and System Package Integration

For C/C++ consumers who use `pkg-config` to discover libraries:

### Generate a .pc file

```bash
# install.sh or build.rs
cat > rg.pc << EOF
prefix=/usr/local
exec_prefix=\${prefix}
libdir=\${exec_prefix}/lib
includedir=\${prefix}/include

Name: rg
Description: Terminal/grid library for roguelikes
Version: 0.1.0
Libs: -L\${libdir} -lrg
Cflags: -I\${includedir}
EOF
```

### Install targets

```makefile
# Makefile (for system packagers)
PREFIX ?= /usr/local
LIBDIR ?= $(PREFIX)/lib
INCLUDEDIR ?= $(PREFIX)/include
PKGCONFIGDIR ?= $(LIBDIR)/pkgconfig

install: build
 install -d $(DESTDIR)$(LIBDIR)
 install -d $(DESTDIR)$(INCLUDEDIR)
 install -d $(DESTDIR)$(PKGCONFIGDIR)
 install -m 755 target/release/librg.so $(DESTDIR)$(LIBDIR)/librg.so.0.1.0
 ln -sf librg.so.0.1.0 $(DESTDIR)$(LIBDIR)/librg.so.0
 ln -sf librg.so.0.1.0 $(DESTDIR)$(LIBDIR)/librg.so
 install -m 644 target/release/librg.a $(DESTDIR)$(LIBDIR)/
 install -m 644 include/rg.h $(DESTDIR)$(INCLUDEDIR)/
 install -m 644 include/rg.hpp $(DESTDIR)$(INCLUDEDIR)/
 sed 's|@PREFIX@|$(PREFIX)|g' rg.pc.in > $(DESTDIR)$(PKGCONFIGDIR)/rg.pc
```

### Usage by downstream C projects

```bash
# Compile
gcc $(pkg-config --cflags rg) -o myapp myapp.c $(pkg-config --libs rg)

# CMake
find_package(PkgConfig REQUIRED)
pkg_check_modules(RG REQUIRED rg)
target_link_libraries(myapp ${RG_LIBRARIES})
target_include_directories(myapp PRIVATE ${RG_INCLUDE_DIRS})
```

---

## 9. Cross-Compilation Strategies

### cross-rs

[cross-rs](https://github.com/cross-rs/cross) provides Docker-based "zero setup" cross compilation.
It is a drop-in replacement for `cargo` that uses pre-built Docker images with the correct
toolchain, sysroot, and QEMU for testing.

```bash
# Install
cargo install cross --git https://github.com/cross-rs/cross

# Build for ARM Linux
cross build --target aarch64-unknown-linux-gnu --release

# Test on emulated architecture (uses QEMU)
cross test --target aarch64-unknown-linux-gnu

# Build the FFI crate specifically
cross build -p rg-ffi --target aarch64-unknown-linux-gnu --release
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
# .cargo/config.toml
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

### Maturin + zig (for Python wheels)

Maturin has built-in zig support for building manylinux-compliant wheels:

```bash
pip install maturin[zig]
maturin build --release --target aarch64-unknown-linux-gnu --zig
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

## 10. CI/CD for Multi-Platform Releases

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
  # ─── Build native libraries for all platforms ───
  build-native:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact_name: librg.so
            static_name: librg.a
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            artifact_name: librg.so
            static_name: librg.a
            use_cross: true
          - os: ubuntu-latest
            target: x86_64-unknown-linux-musl
            artifact_name: librg.so
            static_name: librg.a
            use_cross: true
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact_name: librg.dylib
            static_name: librg.a
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact_name: librg.dylib
            static_name: librg.a
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact_name: rg.dll
            static_name: rg.lib

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross
        if: matrix.use_cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - name: Build FFI crate
        run: |
          if [ "${{ matrix.use_cross }}" = "true" ]; then
            cross build -p rg-ffi --target ${{ matrix.target }} --release
          else
            cargo build -p rg-ffi --target ${{ matrix.target }} --release
          fi
        shell: bash

      - name: Package artifacts
        shell: bash
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/${{ matrix.artifact_name }} dist/ || true
          cp target/${{ matrix.target }}/release/${{ matrix.static_name }} dist/ || true
          cp include/rg.h dist/
          cp include/rg.hpp dist/ || true
          cp LICENSE dist/
          tar czf rg-${{ matrix.target }}.tar.gz -C dist .

      - uses: actions/upload-artifact@v4
        with:
          name: rg-${{ matrix.target }}
          path: rg-${{ matrix.target }}.tar.gz

  # ─── Build Python wheels ───
  build-python:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64
            manylinux: '2014'
          - os: ubuntu-latest
            target: aarch64
            manylinux: '2014'
          - os: macos-latest
            target: x86_64
          - os: macos-latest
            target: aarch64
          - os: windows-latest
            target: x64

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'

      - name: Build wheels
        uses: PyO3/maturin-action@v1
        with:
          target: ${{ matrix.target }}
          manylinux: ${{ matrix.manylinux || 'auto' }}
          args: >
            --release --manifest-path crates/rg-python/Cargo.toml --out dist

      - uses: actions/upload-artifact@v4
        with:
          name: wheels-${{ matrix.os }}-${{ matrix.target }}
          path: dist/*.whl

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
          cd crates/rg-wasm
          wasm-pack build --release --target web --scope rg

      - uses: actions/upload-artifact@v4
        with:
          name: wasm-package
          path: crates/rg-wasm/pkg/

  # ─── Publish everything ───
  publish:
    needs: [build-native, build-python, build-wasm]
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: actions/download-artifact@v4
        with:
          path: artifacts

      # Publish to GitHub Releases
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: artifacts/rg-*/*.tar.gz
          generate_release_notes: true

      # Publish Python wheels to PyPI
      - name: Publish to PyPI
        uses: PyO3/maturin-action@v1
        env:
          MATURIN_PYPI_TOKEN: ${{ secrets.PYPI_TOKEN }}
        with:
          command: upload
          args: --non-interactive artifacts/wheels-*/*.whl

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

      # Publish Rust crate to crates.io
      - name: Publish to crates.io
        run: |
          cargo publish -p rg-core --token ${{ secrets.CARGO_REGISTRY_TOKEN }}
          sleep 30  # Wait for crates.io to index
          cargo publish -p rg-ffi --token ${{ secrets.CARGO_REGISTRY_TOKEN }}
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

  # Verify the C header stays in sync
  check-header:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build -p rg-ffi
      - name: Check header is up to date
        run: git diff --exit-code include/rg.h

  # Test Python bindings
  test-python:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - uses: PyO3/maturin-action@v1
        with:
          command: develop
          args: --manifest-path crates/rg-python/Cargo.toml
      - run: python -c "import rg; t = rg.PyTerminal(80, 24); print(f'{t.width}x{t.height}')"

  # Test WASM build
  test-wasm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown
      - run: cargo install wasm-pack
      - run: cd crates/rg-wasm && wasm-pack test --headless --chrome

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

| Channel         | Tool                   | Artifact                                    | Consumer                           |
| --------------- | ---------------------- | ------------------------------------------- | ---------------------------------- |
| crates.io       | `cargo publish`        | Rust crate (`rg-core`)                      | Rust developers                    |
| GitHub Releases | cargo-dist / custom CI | `.tar.gz` with `.so`/`.dll`/`.dylib` + `.h` | C/C++ developers, system packagers |
| PyPI            | maturin                | Python wheel (`.whl`)                       | Python developers                  |
| npm             | wasm-pack              | WASM + JS glue + `.d.ts`                    | Web/Node.js developers             |
| System packages | Makefile + pkg-config  | `.so` + `.h` + `.pc`                        | System integrators                 |

## Sources

- **Kept**: [Rust Reference: Linkage](https://doc.rust-lang.org/reference/linkage.html) -
  authoritative source for crate-type semantics
- **Kept**: [mozilla/cbindgen](https://github.com/mozilla/cbindgen) - primary tool for C header
  generation
- **Kept**: [PyO3 user guide](https://pyo3.rs/v0.22.0/getting-started) - getting started with
  Rust-Python bindings
- **Kept**: [maturin distribution](https://maturin.rs/distribution) - wheel building, manylinux,
  cross-compilation
- **Kept**: [wasm-pack docs](https://rustwasm.github.io/docs/wasm-pack/) - WASM packaging and npm
  publishing
- **Kept**: [cargo-dist](https://github.com/axodotdev/cargo-dist) - automated binary distribution
- **Kept**: [cross-rs](https://github.com/cross-rs/cross) - Docker-based cross compilation for 50+
  targets
- **Kept**:
  [zig cc blog post](https://andrewkelley.me/post/zig-cc-powerful-drop-in-replacement-gcc-clang.html) -
  deep dive on zig as a cross-compilation linker
- **Kept**: [BearLibTerminal](http://foo.wyrd.name/en:bearlibterminal) - reference for single-DLL
  multi-language distribution
- **Kept**: [metatensor](https://github.com/metatensor/metatensor) - real-world Rust workspace using
  cbindgen + C/C++/Python bindings
- **Dropped**: nickel-org/rust-mustache - not relevant (no FFI, no multi-language distribution)
- **Dropped**: aspect-build/rules_py, rules_js - Bazel-specific, not applicable

## Gaps

1. **Homebrew formula creation**: cargo-dist can auto-generate Homebrew taps, but the details of
   creating a standalone formula for a C library (with headers) vs a binary application need more
   research.
2. **Linux distro packaging** (deb, rpm, AUR): Not covered in depth. Tools like `cargo-deb` and
   `cargo-rpm` exist but may need customization for shared library packaging.
3. **iOS/Android mobile targets**: cross-rs supports Android but iOS cross-compilation from Linux is
   not well supported. Would need Xcode for iOS.
4. **Swift/Kotlin bindings**: If mobile targets matter, generating Swift bindings (via UniFFI or
   similar) and Kotlin/JNI bindings would be another distribution vector.
5. **Version synchronization**: Keeping versions aligned across crates.io, PyPI, npm, and GitHub
   Releases requires tooling (cargo-release, release-plz, or custom scripts).
