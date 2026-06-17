# The Rust Performance Book - Comprehensive Reference

> Source: [The Rust Performance Book](https://nnethercote.github.io/perf-book/) by Nicholas
> Nethercote. First published November 2020. This summary covers all 19 chapters.

---

## 1. Benchmarking

Use real-world workloads where possible. Microbenchmarks and stress tests are useful in moderation.

**Tools:**

- **Built-in bench tests** - unstable, nightly-only.
- **[Criterion](https://github.com/bheisler/criterion.rs)** and
  **[Divan](https://github.com/nvzqz/divan)** - stable, sophisticated alternatives.
- **[Hyperfine](https://github.com/sharkdp/hyperfine)** - general-purpose CLI benchmarking.
- **[Bencher](https://github.com/bencherdev/bencher)** - continuous benchmarking on CI.
- Custom harnesses (e.g. [rustc-perf](https://github.com/rust-lang/rustc-perf/)).

**Metrics:** Wall-time is obvious but high-variance. Tiny memory layout changes cause ephemeral
fluctuations. Lower-variance metrics like instruction counts or cycles can be more stable. No single
summarization method is best across multiple workloads.

**Key advice:** Mediocre benchmarking is far better than none. Start simple, improve over time.

---

## 2. Build Configuration

Build configuration can drastically change performance without code changes.

### Release Builds

Always use `--release` for performance work. Dev builds are 10-100x slower. The output line
`Finished dev [unoptimized + debuginfo]` confirms a dev build.

### Maximizing Runtime Speed

**Codegen units** - Set to 1 for better optimization at the cost of compile time:

```toml
[profile.release]
codegen-units = 1
```

**Link-Time Optimization (LTO):**

- `lto = false` - thin local LTO (default for release)
- `lto = "thin"` - more aggressive, likely improves speed + reduces binary size
- `lto = "fat"` - most aggressive, may or may not improve further
- `lto = "off"` - fully disabled, faster compile, worse runtime

**Alternative allocators** - Can yield large runtime speed and memory improvements:

_jemalloc_ (Linux/Mac):

```toml
[dependencies]
tikv-jemallocator = "0.5"
```

```rust
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
```

On Linux, jemalloc can use transparent huge pages (THP) via
`MALLOC_CONF="thp:always,metadata_thp:always"`.

_mimalloc_ (cross-platform):

```toml
[dependencies]
mimalloc = "0.1"
```

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

**CPU-specific instructions:**

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

Enables AVX and other SIMD instructions for the build machine's CPU. Verify with
`rustc --print cfg -C target-cpu=native`.

**Profile-Guided Optimization (PGO):** Compile, run with sample data to collect profiles, recompile
using that data. 10%+ speedups possible. Use [`cargo-pgo`](https://github.com/Kobzol/cargo-pgo) to
simplify.

### Minimizing Binary Size

- `opt-level = "z"` (smallest) or `"s"` (slightly less aggressive)
- `panic = "abort"` - skip unwind tables
- `strip = "symbols"` - remove symbol table
- See [`min-sized-rust`](https://github.com/johnthagen/min-sized-rust) for advanced techniques

### Minimizing Compile Times

- Use a faster linker: **lld** (default on Linux since Rust 1.90), **mold**, or **wild**. On Mac,
  the system linker is already fast.
- Disable debug info: `debug = false` in `[profile.dev]` (20-40% dev build speedup). Use
  `debug = "line-tables-only"` to keep line info.
- Nightly: `-Zthreads=8` for the experimental parallel front-end.
- Nightly: Cranelift codegen backend for faster dev builds with lower-quality code.

### Recommended Maximum Performance Config

```toml
[profile.release]
codegen-units = 1
lto = "fat"
panic = "abort"
```

Plus an alternative allocator and `-C target-cpu=native` if portability isn't needed.

Use [`cargo-wizard`](https://github.com/Kobzol/cargo-wizard) for guided config selection.

---

## 3. Linting (Clippy)

Run `cargo clippy`. The "Perf" lint group catches sub-optimal patterns automatically.

View all performance lints at the
[Clippy lint list](https://rust-lang.github.io/rust-clippy/master/) (filter by "Perf" group).

Non-perf lints can also help performance. Example: `ptr_arg` suggests changing `&mut Vec<T>` to
`&mut [T]`, reducing indirection.

### Disallowing Types

Use `clippy.toml` to ban types you've replaced with faster alternatives:

```toml
disallowed-types = ["std::collections::HashMap", "std::collections::HashSet"]
```

---

## 4. Profiling

### Profiler Landscape

| Profiler                 | Type                            | Platform                            |
| ------------------------ | ------------------------------- | ----------------------------------- |
| **perf**                 | Hardware perf counters          | Linux                               |
| **Instruments**          | General-purpose                 | macOS                               |
| **Intel VTune**          | General-purpose                 | Win/Linux/Mac                       |
| **AMD uProf**            | General-purpose                 | Win/Linux                           |
| **samply**               | Sampling                        | Mac/Linux/Win                       |
| **flamegraph**           | Flame graphs (uses perf/DTrace) | Linux/DTrace platforms              |
| **Cachegrind/Callgrind** | Instruction counts, cache sim   | Linux/Unix                          |
| **DHAT**                 | Heap allocation profiling       | Linux/Unix (dhat-rs: all platforms) |
| **heaptrack/bytehound**  | Heap profiling                  | Linux                               |
| **counts**               | Ad hoc / frequency profiling    | All                                 |
| **Coz**                  | Causal profiling                | Linux                               |

### Profiling Setup

Enable debug info in release builds:

```toml
[profile.release]
debug = "line-tables-only"
```

Force frame pointers for better stack traces:

```sh
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --release
```

Use `rustfilt` to demangle symbols. Consider `-C symbol-mangling-version=v0` for better profiler
compatibility.

---

## 5. Inlining

Four attributes: none (compiler decides), `#[inline]`, `#[inline(always)]`, `#[inline(never)]`.
Inlining is non-transitive.

**Best candidates:** Very small functions, single-call-site functions.

**Hot/cold splitting:** For large functions with one hot call site, create an `#[inline(always)]`
variant and an `#[inline(never)]` wrapper:

```rust
#[inline(always)]
fn inlined_my_function() { /* ... */ }

#[inline(never)]
fn uninlined_my_function() { inlined_my_function(); }
```

**Outlining:** Move rarely-executed code to `#[cold]` functions to improve hot-path code generation.

Use Cachegrind to verify inlining: inlined functions show no event counts on their first/last lines.

Always re-benchmark after adding inline attributes; effects can be unpredictable.

---

## 6. Hashing

### Alternative Hashers

The default SipHash 1-3 is collision-resistant but slow, especially for short keys. When HashDoS
isn't a concern:

| Crate             | Types                     | Notes                                                     |
| ----------------- | ------------------------- | --------------------------------------------------------- |
| **rustc-hash**    | `FxHashSet`/`FxHashMap`   | Low quality, very fast, best for integers. Used in rustc. |
| **fnv**           | `FnvHashSet`/`FnvHashMap` | Higher quality than FxHash, slightly slower.              |
| **ahash**         | `AHashSet`/`AHashMap`     | Can use AES instructions on supported CPUs.               |
| **nohash_hasher** | -                         | For types with already-random values (identity hash).     |

Real-world example from rustc: switching `fnv` to `fxhash` gave up to 6% speedup. Switching `fxhash`
back to default SipHash caused 4-84% slowdowns.

### Byte-wise Hashing

For types with no padding bytes, hashing raw bytes as a stream (via `zerocopy` or `bytemuck`
`#[derive(ByteHash)]`) can be faster than field-by-field hashing from `#[derive(Hash)]`. Measure
carefully; results depend on the hash function and type layout.

---

## 7. Heap Allocations

Each allocation/deallocation typically involves a global lock and non-trivial data structure work.
Small allocations are not necessarily cheaper than large ones.

### Profiling Allocations

Use **DHAT** to identify hot allocation sites. Reducing allocations by ~10 per million instructions
can yield measurable speedups (~1%).

### `Box`

Simple heap allocation. Box outsized enum variants to shrink the enum type.

### `Rc`/`Arc`

Share values to reduce memory, but overuse can increase allocation rates. `clone()` on `Rc`/`Arc`
only increments the refcount (no allocation).

### `Vec` Optimization

**Growth:** Empty Vec starts at capacity 0, grows to 4, 8, 16, 32, 64... Use `eprintln!` + `counts`
to understand length distributions at hot sites.

**Short Vecs:**

- `SmallVec<[T; N]>` - inline storage for N elements, heap fallback. Slightly slower per-operation
  due to the inline/heap check.
- `ArrayVec` - no heap fallback, faster than SmallVec when max length is known.

**Longer Vecs:** Use `Vec::with_capacity`, `Vec::reserve`, or `Vec::reserve_exact` when size is
known ahead of time.

### `String`

Same optimization strategies as Vec. Alternatives:

- `smartstring` - avoids heap for strings <= 23 ASCII chars (on 64-bit). Drop-in replacement.
- Avoid `format!` when a string literal suffices. Use `format_args!` or `lazy_format` for deferred
  formatting.

### `clone` and `clone_from`

`a.clone_from(&b)` can reuse `a`'s existing allocation. Profile to find hot, unnecessary clones.

### `Cow`

`Cow<'a, T>` holds borrowed or owned data. Avoids unnecessary allocations when data is mostly
borrowed:

```rust
let mut errors: Vec<Cow<'static, str>> = vec![];
errors.push(Cow::Borrowed("static message"));
errors.push(Cow::Owned(format!("dynamic message {}", 42)));
```

`Cow::to_mut()` provides clone-on-write semantics.

### Reusing Collections

Declare collections outside loops, use `clear()` at the end of each iteration to reuse capacity.
Pass `&mut Vec` instead of returning new Vecs.

### Reading Lines from a File

`BufRead::lines()` allocates a `String` per line. Use a workhorse `String` with
`BufRead::read_line()` + `clear()` to reduce allocations to at most a handful.

### Avoiding Regressions

Use `dhat-rs` heap usage testing to write tests that verify expected allocation counts.

---

## 8. Type Sizes

Smaller types = less memory traffic, less cache pressure, better performance.

### Measuring

Use `-Zprint-type-sizes` (nightly) to see exact layout, alignment, padding, and field ordering:

```sh
RUSTFLAGS=-Zprint-type-sizes cargo +nightly build --release
```

The [`top-type-sizes`](https://crates.io/crates/top-type-sizes) crate provides compact output.

Types > 128 bytes are copied via `memcpy` instead of inline code. Shrink hot types below this
threshold.

### Techniques

**Field ordering:** The compiler auto-reorders fields for minimal size (unless `#[repr(C)]`). No
manual ordering needed.

**Smaller enums:** Box outsized variants:

```rust
enum A {
    X,
    Y(i32),
    Z(Box<(i32, LargeType)>),  // Box the large variant
}
```

**Smaller integers:** Use `u32`, `u16`, or `u8` for indices instead of `usize` when the range
allows, coercing at use points.

**Boxed slices:** Convert finalized `Vec` to `Box<[T]>` via `Vec::into_boxed_slice()` to save one
word (no capacity field).

**`ThinVec`:** Stores length and capacity in the heap allocation itself. `size_of::<ThinVec<T>>()`
is one word. Good for often-empty vecs in oft-instantiated types.

### Regression Prevention

```rust
#[cfg(target_arch = "x86_64")]
static_assertions::assert_eq_size!(HotType, [u8; 64]);
```

---

## 9. Standard Library Types

### `Vec`

- Zero-fill with `vec![0; n]` (uses OS assistance, fastest approach).
- Use `swap_remove` (O(1)) instead of `remove` (O(n)) when order doesn't matter.
- Use `retain` for efficient multi-element removal.

### `Option` and `Result`

Prefer lazy variants to avoid unnecessary computation:

- `ok_or_else` over `ok_or`
- `map_or_else` over `map_or`
- `unwrap_or_else` over `unwrap_or`

### `Rc`/`Arc`

`Rc::make_mut` / `Arc::make_mut` provide clone-on-write: clones only when refcount > 1.

### Synchronization

`parking_lot` provides alternative `Mutex`, `RwLock`, `Condvar`, `Once`. The std versions have
improved significantly; benchmark before switching. Use Clippy `disallowed_types` to enforce
consistency.

---

## 10. Iterators

- Avoid unnecessary `collect()`. Return `impl Iterator<Item=T>` instead of `Vec<T>` when the caller
  just iterates.
- Use `extend` to append an iterator to an existing collection instead of `collect` + `append`.
- Implement `size_hint` / `ExactSizeIterator::len` on custom iterators for fewer allocations during
  `collect`/`extend`.
- `chain` can be slower than a single iterator on hot paths.
- Prefer `filter_map` over `filter` + `map`.
- Use `chunks_exact` over `chunks` when the chunk size divides evenly (same for `rchunks_exact`,
  mutable variants).
- `iter().copied()` can generate better LLVM code than `iter()` for small types like integers.

---

## 11. Bounds Checks

Default container accesses include bounds checks. Safe ways to elide them:

1. Use iteration instead of direct indexing.
2. Slice the `Vec` before the loop, index into the slice.
3. Add assertions on index ranges before the loop.
4. Last resort: `get_unchecked` / `get_unchecked_mut` (unsafe).

See the [Bounds Check Cookbook](https://github.com/Shnatsel/bounds-check-cookbook/) for patterns.

---

## 12. I/O

### Stdout/Stderr Locking

`println!` locks stdout on every call. For repeated output, lock once:

```rust
use std::io::Write;
let mut lock = std::io::stdout().lock();
for line in lines {
    writeln!(lock, "{}", line)?;
}
```

### Buffering

File I/O is unbuffered by default. Wrap with `BufReader`/`BufWriter`:

```rust
let mut out = BufWriter::new(File::create("test.txt")?);
```

Call `flush()` explicitly to catch errors (auto-flush on drop swallows errors).

Combine locking and buffering for heavy stdout writes.

### Raw Bytes

Skip UTF-8 validation overhead by reading as raw bytes with `BufRead::read_until`. Use `bstr` crate
for byte-string processing.

---

## 13. Logging and Debugging

Ensure no unnecessary work is done when logging/debugging is disabled. Logging code can collect data
eagerly even when the log level is off.

Use `debug_assert!` instead of `assert!` for hot assertions that aren't safety-critical.
`debug_assert!` only runs in dev builds.

---

## 14. Wrapper Types

When multiple values wrapped in `RefCell`, `Mutex`, `Arc`, etc. are accessed together, combine them
under a single wrapper:

```rust
// Before: two locks
struct S { x: Arc<Mutex<u32>>, y: Arc<Mutex<u32>> }
// After: one lock
struct S { xy: Arc<Mutex<(u32, u32)>> }
```

---

## 15. Machine Code Inspection

For very hot code, inspect generated assembly:

- **[Compiler Explorer (godbolt.org)](https://godbolt.org/)** - for small snippets.
- **[cargo-show-asm](https://github.com/pacak/cargo-show-asm)** - for full projects.
- **`core::arch`** - architecture-specific SIMD intrinsics.

---

## 16. Parallelism

Rust has excellent safe parallelism support. Key resources:

- **[rayon](https://crates.io/crates/rayon)** - data parallelism (parallel iterators).
- **[crossbeam](https://crates.io/crates/crossbeam)** - scoped threads, channels, concurrent data
  structures.
- **[Rust Atomics and Locks](https://marabos.nl/atomics/)** - book on low-level concurrency.
- For SIMD/data parallelism, see
  [the state of SIMD in Rust in 2025](https://shnatsel.medium.com/the-state-of-simd-in-rust-in-2025-32c263e5f53d).

---

## 17. General Tips

1. **Optimize only hot code.** Optimized code is more complex; spend effort where it matters.
2. **Algorithm/data structure changes** yield the biggest wins, not low-level tweaks.
3. **Minimize cache misses and branch mispredictions.**
4. **Many small speedups compound.** No single one is noticeable; together they matter.
5. **Use multiple profilers.** Each has different strengths.
6. **Two ways to speed up a hot function:** make it faster, or call it less.
7. **Lazy/on-demand computation** is often a win. Don't compute what you don't need.
8. **Special-case common cases.** Handle 0, 1, or 2-element collections separately when small sizes
   dominate.
9. **Compact representations** for common values with a fallback table for rare values.
10. **Measure case frequencies** and handle the most common first.
11. **Small caches** in front of data structures can help with high-locality lookups.
12. **Comment non-obvious optimizations** with profiling data that motivated them.

---

## 18. Compile Times

### Visualization

```sh
cargo build --timings
```

Generates an HTML Gantt chart showing crate compilation dependencies and parallelism.

### Reducing Macro Bloat

Use `-Zmacro-stats` (nightly) to measure code generated by proc macros and declarative macros:

```sh
cargo +nightly rustc -- -Zmacro-stats
```

Use `cargo-expand` to see generated code. Replace heavy macros with lighter alternatives or
equivalent `match` expressions.

### Reducing LLVM IR Bloat

Use [`cargo llvm-lines`](https://github.com/dtolnay/cargo-llvm-lines/) to find functions generating
the most LLVM IR. Generic functions are the usual culprits.

**Fix:** Extract non-generic inner functions:

```rust
pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    fn inner(path: &Path) -> io::Result<Vec<u8>> {
        // all the actual work, only instantiated once
    }
    inner(path.as_ref())
}
```

Replace generic combinators like `Option::map` and `Result::map_err` with `match` in hot paths to
reduce monomorphization.

---

## Quick Reference: Crates Worth Knowing

| Purpose                        | Crate                  |
| ------------------------------ | ---------------------- |
| Allocator (Linux/Mac)          | `tikv-jemallocator`    |
| Allocator (cross-platform)     | `mimalloc`             |
| Hasher (fastest, low quality)  | `rustc-hash`           |
| Hasher (AES-accelerated)       | `ahash`                |
| Hasher (identity)              | `nohash_hasher`        |
| Small vec                      | `smallvec`, `arrayvec` |
| Thin vec                       | `thin_vec`             |
| Smart string                   | `smartstring`          |
| Sync primitives                | `parking_lot`          |
| Benchmarking                   | `criterion`, `divan`   |
| CLI benchmarking               | `hyperfine`            |
| Heap profiling (all platforms) | `dhat-rs`              |
| Flame graphs                   | `flamegraph`           |
| Sampling profiler              | `samply`               |
| Build config wizard            | `cargo-wizard`         |
| PGO helper                     | `cargo-pgo`            |
| Assembly viewer                | `cargo-show-asm`       |
| LLVM IR analysis               | `cargo-llvm-lines`     |
| Parallelism                    | `rayon`, `crossbeam`   |
| Static assertions              | `static_assertions`    |
| Byte hashing                   | `zerocopy`, `bytemuck` |
