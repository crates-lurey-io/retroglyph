# Research: Linux Kernel Rust Coding Guidelines

## Summary

The Linux kernel's Rust coding guidelines enforce `rustfmt` with default settings (4-space indent,
no tabs), vertical import formatting via trailing `//` comments, mandatory `// SAFETY:` comments on
every `unsafe` block, and a near-total prohibition on panicking. The kernel uses a layered
architecture where leaf modules (drivers) never call C bindings directly; instead, subsystem
abstractions in `rust/kernel/` encapsulate all unsafe FFI, providing safe Rust APIs. As of kernel
6.17+, real Rust drivers are in mainline, including the ASIX PHY driver (first Rust driver, merged
6.8) and the Nova GPU driver skeleton (merged 6.15).

## Findings

### 1. Formatting: rustfmt with Default Settings

The kernel mandates `rustfmt` with its default configuration. This means 4-space indentation (not
tabs, unlike the rest of the kernel's C code), standard Rust brace style, and all the defaults from
the official Rust style guide. The rationale is explicit: occasional contributors should not need to
learn another style, and reviewers should not waste patch roundtrips on formatting.
[Source](https://docs.kernel.org/rust/coding-guidelines.html)

`rustfmt` can be run on the whole tree:

```bash
make LLVM=1 rustfmt        # format all Rust sources
make LLVM=1 rustfmtcheck   # check formatting (CI-friendly, prints diff)
```

Like `clang-format` for C, `rustfmt` operates on individual files and does not require a kernel
configuration. It can even work on broken code.

### 2. Vertical Import Formatting

The kernel overrides `rustfmt`'s default import condensing because condensed imports cause frequent
merge conflicts. Instead, a vertical layout is enforced: one item per line, braces required for any
nested list, and a **trailing empty comment (`//`)** on the last item to force `rustfmt` to preserve
the vertical layout. [Source](https://docs.kernel.org/rust/coding-guidelines.html)

```rust
// Correct kernel style:
use crate::{
    example1,
    example2::{
        example3,
        example4,
        example5, //
    },
    example6,
    example7,
    example8::example9, //
};

// Also valid for single-item imports (minimizes diffs in patch series):
use crate::{
    example1, //
};
```

The trailing `//` is a temporary workaround. The long-term goal is to get `rustfmt` to natively
support this style so the comments can be removed. Not all existing code has been migrated yet, but
new code must use this style.

### 3. Documentation Requirements

Rust kernel code uses `rustdoc` (Markdown), not `kernel-doc`. The conventions are strict:

- **First paragraph**: a single sentence summarizing what the item does.
- **`# Safety` section**: mandatory for all `unsafe fn`. Documents the preconditions that callers
  must uphold.
- **`# Panics` section**: required if a function can panic, documenting the conditions.
- **`# Examples` section**: encouraged; examples are compiled and run as KUnit tests.
- **Cross-references**: Rust items must be linked with rustdoc's `[backtick]` syntax. The kernel
  also supports `srctree/` links for references to C headers:

```rust
//! C header: [`include/linux/printk.h`](srctree/include/linux/printk.h)
```

**SAFETY comments** are the most critical documentation rule: every `unsafe` block must be preceded
by a `// SAFETY:` comment explaining why the contained code is sound. This is not optional even when
the reason seems trivial, because the comment's purpose is to make all implicit constraints
explicit, ensuring nothing is missed during review.

```rust
// SAFETY: The safety contract must be upheld by the caller.
None => unsafe { hint::unreachable_unchecked() },
```

The kernel has also proposed a formal "safety standard" (RFC patches by Benno Lossin) that would
further systematize how safety requirements and justifications are documented, with the goal that
every `unsafe` operation's requirements are matched by a corresponding justification at the call
site. [Source](https://lkml.indiana.edu/hypermail/linux/kernel/2407.2/02426.html)

### 4. Naming Conventions: Mirroring C APIs

Rust kernel code follows standard Rust naming (snake_case functions, CamelCase types,
SCREAMING_SNAKE_CASE constants) but with an important kernel-specific rule: **names should stay as
close to the C originals as possible** to reduce cognitive overhead when switching between C and
Rust. [Source](https://docs.kernel.org/rust/coding-guidelines.html)

Adjustments from C to Rust idioms are allowed:

- Casing adapts to Rust conventions.
- Redundant namespacing is stripped, since Rust modules and types provide their own namespacing.

```rust
// C:
// #define GPIO_LINE_DIRECTION_IN  0
// #define GPIO_LINE_DIRECTION_OUT 1

// Rust (idiomatic):
pub mod gpio {
    pub enum LineDirection {
        In = bindings::GPIO_LINE_DIRECTION_IN as _,
        Out = bindings::GPIO_LINE_DIRECTION_OUT as _,
    }
}
// Access: gpio::LineDirection::In
// NOT: gpio::gpio_line_direction::GPIO_LINE_DIRECTION_IN
```

Macros that already have idiomatic names in C (like `pr_info`) keep the same name in Rust.

### 5. Panic Policy: Near-Zero Tolerance

Panicking is treated as an extreme measure. The guidelines state: **"panicking should be very rare
and used only with a good reason. In almost all cases, a fallible approach should be used, typically
returning a `Result`."** [Source](https://docs.kernel.org/rust/coding-guidelines.html)

The kernel operates in `#![no_std]`, linking only `core` (not `std` or `alloc` from the standard
library). The kernel provides its own allocator and collection types (like `KVec`) that take
allocation flags (e.g., `GFP_KERNEL`) and return `Result` instead of panicking on OOM.

```rust
let mut numbers = KVec::new();
numbers.push(72, GFP_KERNEL)?;  // Returns Err on OOM, never panics
```

Error handling uses `kernel::error::Result<T>`, which is
`core::result::Result<T, kernel::error::Error>`. The `Error` type wraps kernel errno codes
(`EINVAL`, `ENOMEM`, etc.) and can only hold valid errno values (i.e., `>= -MAX_ERRNO && < 0`).
[Source](https://kernel.org/doc/rustdoc/latest/kernel/error/struct.Error.html)

Doctests use the `?` operator rather than `unwrap()` or `expect()` to model real error handling
patterns. If a function can panic, it must document the conditions under a `# Panics` section.

### 6. Lint Preferences: `#[expect]` over `#[allow]`

The kernel prefers `#[expect(lint)]` over `#[allow(lint)]`. The `expect` attribute causes a compiler
warning if the suppressed lint is no longer triggered, preventing stale suppressions from
accumulating. [Source](https://docs.kernel.org/rust/coding-guidelines.html)

`#[allow]` is acceptable in three cases:

1. **Conditional compilation**: the lint fires for some `CONFIG_*` values but not others.
2. **Macros**: different invocations may or may not trigger the lint.
3. **Architecture-dependent code**: e.g., an `as` cast to a C FFI type that changes size across
   architectures.

For complex conditional cases, `cfg_attr(not(CONFIG_X), expect(dead_code))` is possible but usually
not worth the complexity when more than one or two configurations are involved.

### 7. C FFI Type Rules

The kernel provides its own type aliases (`c_int`, `c_char`, etc.) in the `kernel` prelude. Code
must **not** use `core::ffi` aliases, because they may not map to the correct types for the kernel's
target. These aliases should be used as bare identifiers (single-segment paths), not fully
qualified. [Source](https://docs.kernel.org/rust/coding-guidelines.html)

```rust
fn f(p: *const c_char) -> c_int {
    // ...
}
```

### 8. Build System Integration (Kbuild/Kconfig)

Rust integrates into the kernel's existing Kbuild system:

- **`CONFIG_RUST`** enables Rust support (visible only when a valid Rust toolchain is detected).
- **`make LLVM=1 rustavailable`** checks if all requirements are met and explains why if not.
- The build system **cross-compiles `core`** from the Rust standard library source (`rust-src`
  component).
- **`bindgen`** auto-generates Rust bindings from C headers listed in
  `rust/bindings/bindings_helper.h`. For inline C functions or non-trivial macros, small wrapper
  functions go in `rust/helpers/`.
- **`make LLVM=1 rustdoc`** generates HTML documentation.
- **`make LLVM=1 rust-analyzer`** generates `rust-project.json` for IDE integration.
- **`make LLVM=1 rustfmt`** formats all Rust sources.
- **Clippy** runs with `CLIPPY=1` added to the make invocation.
- **Conditional compilation** uses `#[cfg(CONFIG_X)]`, `#[cfg(CONFIG_X="y")]`,
  `#[cfg(CONFIG_X="m")]`.

Supported architectures (as of 6.17+): x86_64, arm64 (LE), arm (ARMv7 LE), loongarch, riscv64 (LLVM
only), and um. [Source](https://docs.kernel.org/rust/arch-support.html)

### 9. The Kernel's Approach to Unsafe Code

The kernel enforces a strict layered architecture for unsafe code:

```
drivers/fs/...          rust/kernel/            rust/bindings/
(Leaf modules)          (Abstractions)          (Auto-generated bindings)
    |                       |                       |
    +--- Safe calls --->    +--- Unsafe calls --->  +--- bindgen from C headers
```

**Key rules:**

- **Leaf modules (drivers, filesystems) must never use C bindings directly.** They call safe
  abstractions provided by `rust/kernel/`.
- **Abstractions** in `rust/kernel/` encapsulate all `unsafe` C FFI calls, exposing safe, idiomatic
  Rust APIs.
- **Soundness contract**: if abstractions are correct ("sound") and all `unsafe` blocks/impls
  respect their documented safety contracts, then safe Rust code cannot introduce undefined
  behavior.
- **One `unsafe` operation per `unsafe` block** is best practice, making each SAFETY comment
  unambiguous.
- **Every `unsafe` block requires a `// SAFETY:` comment** at the call site that justifies why the
  safety contract is satisfied.
- **Every `unsafe fn` requires a `# Safety` doc section** that specifies its preconditions.

Abstractions also convert C patterns to Rust idioms: C resource acquire/release becomes
constructors/`Drop`, C integer error codes become `Result<T, Error>`, etc.
[Source](https://docs.kernel.org/rust/general-information.html)

### 10. Examples of Rust Kernel Modules/Drivers

**Merged in mainline:**

| Driver/Module        | Kernel Version | Description                                                                                                                                                               |
| -------------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ASIX PHY driver      | 6.8-rc1        | First Rust driver merged. Equivalent to `drivers/net/phy/ax88796b.c`. [Source](https://rust-for-linux.com/asix-phy-driver)                                                |
| Nova Core (skeleton) | 6.15           | Rust-based NVIDIA GPU driver skeleton in `drivers/gpu/nova-core/`. Intended successor to Nouveau for GSP-based GPUs. [Source](https://rust-for-linux.com/nova-gpu-driver) |
| Nova Core (expanded) | 6.17           | Additional abstractions: aux bus, devres, PCI, DRM infrastructure. [Source](https://www.phoronix.com/news/Linux-6.17-NOVA-Driver)                                         |

**Sample modules in `samples/rust/`:**

| Sample                    | Description                                                                                             |
| ------------------------- | ------------------------------------------------------------------------------------------------------- |
| `rust_minimal.rs`         | Minimal module demonstrating `module!` macro, `Module` trait, `Drop`, parameters                        |
| `rust_driver_pci.rs`      | PCI driver for QEMU's `pci-testdev`, demonstrating PCI probe/unbind, BAR mapping, register abstractions |
| `rust_driver_platform.rs` | Platform driver sample                                                                                  |
| `rust_misc_device.rs`     | Miscellaneous device with ioctl                                                                         |
| `rust_driver_i2c.rs`      | I2C driver with ACPI/OF/Legacy ID tables                                                                |
| `rust_print.rs`           | Printing macros (`pr_info!`, `pr_err!`, etc.)                                                           |

**Minimal module example** (`rust_minimal.rs`):

```rust
use kernel::prelude::*;

module! {
    type: RustMinimal,
    name: "rust_minimal",
    authors: ["Rust for Linux Contributors"],
    description: "Rust minimal sample",
    license: "GPL",
}

struct RustMinimal {
    numbers: KVec<i32>,
}

impl kernel::Module for RustMinimal {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust minimal sample (init)\n");
        let mut numbers = KVec::new();
        numbers.push(72, GFP_KERNEL)?;
        Ok(RustMinimal { numbers })
    }
}

impl Drop for RustMinimal {
    fn drop(&mut self) {
        pr_info!("Rust minimal sample (exit)\n");
    }
}
```

### 11. Kernel-Specific Rust Patterns

**`module!` macro**: the entry point for every Rust kernel module. Declares metadata (name, authors,
description, license, parameters) and the implementing type. The type must implement
`kernel::Module` (with `fn init() -> Result<Self>`), and cleanup happens via `Drop`. For
bus-specific drivers, convenience macros exist: `module_pci_driver!`, `module_platform_driver!`,
`module_i2c_driver!`, etc.

**Pin-init**: the kernel uses `#[pin_data]` and `PinInit` for in-place initialization of
self-referential or pinned types, since `no_std` Rust cannot rely on `Box::pin()`. The `pin_init!`
and `try_pin_init!` macros provide fallible pinned initialization.

**`KVec` and fallible allocation**: the kernel's vector type (`KVec<T>`) takes an allocation flag on
every operation (`GFP_KERNEL`, `GFP_ATOMIC`, etc.), returning `Result` instead of panicking. There
is no implicit OOM panic.

**Device resource management (`Devres`)**: wraps resources that are automatically freed when a
device is unbound, mirroring the C `devm_*` pattern.

**Testing via KUnit**: Rust doctests are compiled to kernel objects and run as KUnit test suites.
The `#[kunit_tests(suite_name)]` attribute creates standard `#[test]` functions that map to KUnit.
`assert!` and `assert_eq!` are redirected to KUnit assertions rather than panicking.

**`register!` macro**: defines typed register abstractions with bitfield accessors, used for MMIO
register access with compile-time offset checking.

**`kernel` prelude**: re-exports the most commonly needed items (`Result`, `Error`, `ThisModule`,
`pr_info!`, `module!`, `KVec`, `GFP_KERNEL`, `Pin`, `pin_init!`, etc.) so modules can
`use kernel::prelude::*`.

**`srctree/` links**: documentation can cross-reference C headers relative to the kernel source
tree.

**Conditional compilation**: `#[cfg(CONFIG_X)]` maps directly to Kconfig options. For complex
conditions (e.g., numerical comparisons), define a new Kconfig `def_bool` symbol.

## Sources

- **Kept:**
  - [Coding Guidelines, kernel.org](https://docs.kernel.org/rust/coding-guidelines.html) - primary
    source for all formatting, documentation, naming, lint, and FFI rules
  - [General Information, kernel.org](https://docs.kernel.org/rust/general-information.html) -
    abstractions vs bindings architecture, conditional compilation, no_std
  - [Quick Start, kernel.org](https://docs.kernel.org/rust/quick-start.html) - toolchain
    requirements, build system integration
  - [Testing, kernel.org](https://docs.kernel.org/rust/testing.html) - KUnit integration, doctest
    compilation, #[test] mapping
  - [Architecture Support, kernel.org](https://docs.kernel.org/rust/arch-support.html) - supported
    architectures table
  - [kernel crate rustdoc](https://rust.docs.kernel.org/kernel/index.html) - full API surface of the
    kernel crate
  - [kernel::error module](https://kernel.org/doc/rustdoc/latest/kernel/error/index.html) - Error
    type, Result alias, errno codes
  - [rust_minimal.rs (GitHub raw)](https://raw.githubusercontent.com/Rust-for-Linux/linux/refs/heads/rust-next/samples/rust/rust_minimal.rs) -
    minimal module sample
  - [rust_driver_pci.rs (GitHub raw)](https://raw.githubusercontent.com/Rust-for-Linux/linux/refs/heads/rust-next/samples/rust/rust_driver_pci.rs) -
    PCI driver sample with register abstractions
  - [ASIX PHY Driver, rust-for-linux.com](https://rust-for-linux.com/asix-phy-driver) - first merged
    Rust driver
  - [Nova GPU Driver, rust-for-linux.com](https://rust-for-linux.com/nova-gpu-driver) - NVIDIA GPU
    driver project status
  - [Safety standard RFC, LKML](https://lkml.indiana.edu/hypermail/linux/kernel/2407.2/02426.html) -
    proposed safety documentation standard
  - [Phoronix: Linux 6.17 NOVA](https://www.phoronix.com/news/Linux-6.17-NOVA-Driver) - Nova driver
    expansion timeline

- **Dropped:**
  - mintlify.com mirror of kernel docs (third-party mirror, no original content)
  - rust-for-linux.github.io/docs (older pre-merge documentation, superseded by kernel.org rustdoc)
  - Various LKML thread fragments (used for context but not as primary sources)

## Gaps

1. **Formal safety standard**: the RFC patches for a formal `Documentation/rust/safety-standard/`
   have not been merged as of this research. The current rules are documented in the coding
   guidelines but not as a standalone standard document.
2. **Macro hygiene details**: the `module!` macro's internal implementation and soundness fixes
   (e.g., the `__init`/`__exit` visibility issue) are in flux. Specific macro rules for writing new
   proc macros are not well-documented publicly.
3. **Performance benchmarks**: no published benchmarks comparing Rust kernel modules to their C
   equivalents were found in official kernel documentation.
4. **Clippy configuration**: the specific Clippy lints enabled/disabled for kernel builds are
   configured in the build system but not documented in the coding guidelines page.
5. **Alloc crate status**: the kernel previously used a vendored `alloc` crate, but recent versions
   appear to have moved away from it. The current state of collection types and allocator
   integration is evolving.
