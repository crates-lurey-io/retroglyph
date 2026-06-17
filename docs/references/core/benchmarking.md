# Benchmarking and Performance Measurement

Reference guide for benchmarking a Rust terminal/grid rendering library. Covers micro-benchmarks,
GPU timing, latency measurement, memory profiling, flame graphs, WASM profiling, CI regression
testing, and concrete code examples.

## Table of Contents

1. [Benchmark Framework: Criterion vs Divan](#1-benchmark-framework-criterion-vs-divan)
2. [What to Benchmark](#2-what-to-benchmark)
3. [GPU Frame Timing](#3-gpu-frame-timing)
4. [Input-to-Display Latency](#4-input-to-display-latency)
5. [Memory Profiling](#5-memory-profiling)
6. [Flame Graphs](#6-flame-graphs)
7. [WASM Profiling](#7-wasm-profiling)
8. [Prior Art: Terminal Emulator Benchmarks](#8-prior-art-terminal-emulator-benchmarks)
9. [CI Regression Testing](#9-ci-regression-testing)
10. [Concrete Setup and Code Examples](#10-concrete-setup-and-code-examples)

---

## 1. Benchmark Framework: Criterion vs Divan

### Recommendation

Use **Divan** for new projects. It has a simpler API, built-in allocation profiling, generic
benchmarks, multi-threaded contention testing, and better sample size scaling for noisy CI
environments. Fall back to Criterion if you need HTML report generation, baseline comparison (Divan
doesn't have this yet), or compatibility with existing tooling that parses Criterion output.

### Criterion

[criterion.rs](https://github.com/bheisler/criterion.rs) (5.5k stars) is the established Rust
benchmarking library, ported from Haskell's Criterion. It uses statistical analysis (linear
regression, bootstrap confidence intervals) to detect performance changes.

Key features:

- Statistical confidence intervals for detecting real changes vs noise
- HTML report generation with charts (violin plots, PDFs, regression lines)
- Baseline comparison: saves previous run, compares against it
- Throughput measurement via `Throughput::Bytes(n)` / `Throughput::Elements(n)`
- Async benchmark support
- Custom measurement trait (can plug in CPU counters, GPU timers, etc.)
- `--profile-time` flag for integration with external profilers

Downsides:

- Verbose API: requires `criterion_group!` / `criterion_main!` macros
- No built-in allocation tracking
- No generic type/const parameter benchmarks
- Slow compile times (depends on plotters/gnuplot)
- Maintenance has moved to [criterion-rs org](https://github.com/criterion-rs/criterion.rs); the
  original repo is less active

### Divan

[Divan](https://github.com/nvzqz/divan) (1000+ stars) is a newer framework focused on simplicity and
power.

Key features:

- `#[divan::bench]` attribute, similar to `#[test]` -- register benchmarks anywhere
- Module tree hierarchy reflected in output formatting
- Generic benchmarks: `#[divan::bench(types = [Vec<i32>, HashMap<i32, i32>])]`
- Const generic benchmarks: `#[divan::bench(consts = [64, 128, 256])]`
- Built-in `AllocProfiler` that counts allocations and bytes per benchmark
- Multi-threaded benchmarks: `#[divan::bench(threads = [1, 4, 8])]`
- Throughput counters: `BytesCount`, `CharsCount`, `ItemsCount`
- Deferred drop: returned values are not dropped during timing
- Sample size scaling based on timer precision (adapts to noisy CI)
- CPU timestamp counter support via `DIVAN_TIMER=tsc`
- `Bencher` passed by-value (builder pattern prevents misuse)

Downsides:

- No HTML reports or chart generation (terminal-only output)
- No baseline comparison yet (planned)
- No machine-readable output (JSON/CSV planned)
- Newer, smaller ecosystem

### Comparison Table

| Feature               | Criterion                        | Divan                               |
| --------------------- | -------------------------------- | ----------------------------------- |
| API style             | Macro-based groups               | `#[divan::bench]` attribute         |
| Statistics            | Confidence intervals, regression | Min/max/median/mean, sample scaling |
| HTML reports          | Yes                              | No (planned)                        |
| Baseline comparison   | Yes                              | No (planned)                        |
| Allocation profiling  | No                               | Yes (`AllocProfiler`)               |
| Generic benchmarks    | No                               | Yes (types + consts)                |
| Multi-threaded        | No                               | Yes (`threads` option)              |
| Throughput counters   | `Throughput` enum                | `BytesCount`, `ItemsCount`, etc.    |
| Async support         | Yes                              | Planned                             |
| CI friendliness       | Moderate (noisy)                 | Good (sample scaling)               |
| Compile time          | Slow (plotters)                  | Fast                                |
| Ecosystem integration | Bencher, github-action-benchmark | Bencher                             |

---

## 2. What to Benchmark

For a terminal grid rendering library, benchmark these layers from pure CPU work through GPU
submission.

### 2.1 Cell Write Throughput

Measure how fast individual cells can be written to the grid buffer.

```rust
#[divan::bench(consts = [80, 132, 200])]
fn cell_write_sequential<const COLS: usize>(bencher: divan::Bencher) {
    let mut grid = Grid::new(24, COLS);
    bencher
        .counter(divan::counter::ItemsCount::new(24 * COLS))
        .bench(|| {
            for row in 0..24 {
                for col in 0..COLS {
                    grid.write_cell(row, col, Cell::new('A', Style::default()));
                }
            }
        });
}
```

Target: millions of cells/second. This tests the hot path for PTY output processing.

### 2.2 Full Grid Clear and Fill

Measure bulk operations on the entire grid buffer.

```rust
#[divan::bench(args = [(80, 24), (132, 50), (200, 60)])]
fn grid_clear_and_fill(bencher: divan::Bencher, (cols, rows): (usize, usize)) {
    let mut grid = Grid::new(rows, cols);
    bencher
        .counter(divan::counter::ItemsCount::new(rows * cols))
        .bench(|| {
            grid.clear(Cell::default());
            for row in 0..rows {
                for col in 0..cols {
                    grid.write_cell(row, col, Cell::new('X', Style::default()));
                }
            }
        });
}
```

### 2.3 Diff Computation

Measure the damage tracking / dirty region computation between frames.

```rust
#[divan::bench]
fn diff_sparse_changes(bencher: divan::Bencher) {
    let rows = 50;
    let cols = 200;
    let old = Grid::filled(rows, cols, Cell::new(' ', Style::default()));
    let mut new = old.clone();
    // Simulate sparse edits: change 5% of cells
    let mut rng = fastrand::Rng::with_seed(42);
    for _ in 0..(rows * cols / 20) {
        let r = rng.usize(..rows);
        let c = rng.usize(..cols);
        new.write_cell(r, c, Cell::new('X', Style::bold()));
    }
    bencher.bench(|| {
        grid_diff(&old, &new)
    });
}

#[divan::bench]
fn diff_full_repaint(bencher: divan::Bencher) {
    let old = Grid::filled(50, 200, Cell::new(' ', Style::default()));
    let new = Grid::filled(50, 200, Cell::new('X', Style::bold()));
    bencher.bench(|| {
        grid_diff(&old, &new)
    });
}
```

### 2.4 Text Layout and Wrapping

Measure Unicode segmentation, line wrapping, and bidirectional text handling.

```rust
#[divan::bench(types = [&str, String])]
fn text_layout_ascii<'a, T: AsRef<str>>(bencher: divan::Bencher) {
    let line = "Hello, this is a typical terminal line with some content.\n".repeat(100);
    bencher
        .counter(divan::counter::BytesCount::of_str(&line))
        .bench(|| {
            layout_text(line.as_ref(), 80)
        });
}

#[divan::bench]
fn text_layout_unicode_wide(bencher: divan::Bencher) {
    // CJK characters are 2 columns wide
    let line = "日本語のテキスト処理のベンチマーク".repeat(50);
    bencher
        .counter(divan::counter::BytesCount::of_str(&line))
        .bench(|| {
            layout_text(&line, 80)
        });
}
```

### 2.5 Glyph Atlas Lookup

Measure the glyph cache hit/miss path.

```rust
#[divan::bench]
fn glyph_cache_hit(bencher: divan::Bencher) {
    let mut atlas = GlyphAtlas::new(1024, 1024);
    // Pre-warm with ASCII
    for c in 0x20u32..0x7F {
        atlas.rasterize(char::from_u32(c).unwrap(), &font, 14.0);
    }
    bencher.bench(|| {
        // All lookups should be cache hits
        for c in 0x20u32..0x7F {
            divan::black_box(atlas.lookup(char::from_u32(c).unwrap(), &font, 14.0));
        }
    });
}

#[divan::bench]
fn glyph_cache_miss(bencher: divan::Bencher) {
    bencher
        .with_inputs(|| GlyphAtlas::new(1024, 1024))
        .bench_refs(|atlas| {
            // Cold cache: every lookup is a miss
            for c in 0x20u32..0x7F {
                divan::black_box(atlas.rasterize(char::from_u32(c).unwrap(), &font, 14.0));
            }
        });
}
```

### 2.6 Buffer Serialization (ANSI/VT Escape Output)

Measure the generation of escape sequences from the grid state.

```rust
#[divan::bench(args = [(80, 24), (200, 60)])]
fn serialize_to_ansi(bencher: divan::Bencher, (cols, rows): (usize, usize)) {
    let grid = make_colorful_grid(rows, cols);
    let mut buf = Vec::with_capacity(rows * cols * 20);
    bencher
        .counter(divan::counter::BytesCount::new(rows * cols * 20)) // approximate
        .bench(|| {
            buf.clear();
            grid.serialize_ansi(&mut buf);
            divan::black_box(&buf);
        });
}
```

---

## 3. GPU Frame Timing

### wgpu Timestamp Queries

wgpu supports GPU-side timestamp queries through the WebGPU `QuerySet` API. This measures actual GPU
execution time, not CPU submission time.

**Requirements:**

- Device must have `Features::TIMESTAMP_QUERY` enabled
- Not all backends/hardware support it (check `adapter.features()`)

**Steps:**

1. Create a `QuerySet` with `QueryType::Timestamp`:

```rust
let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
    label: Some("timestamp_queries"),
    ty: wgpu::QueryType::Timestamp,
    count: 2, // start + end
});
```

2. Attach to render pass via `RenderPassTimestampWrites`:

```rust
let mut encoder = device.create_command_encoder(&Default::default());
{
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("main_pass"),
        color_attachments: &[/* ... */],
        depth_stencil_attachment: None,
        timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
            query_set: &query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }),
        occlusion_query_set: None,
    });
    // ... draw calls ...
}
```

3. Resolve queries to a buffer:

```rust
let resolve_buf = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("timestamp_resolve"),
    size: 2 * std::mem::size_of::<u64>() as u64,
    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
    mapped_at_creation: false,
});
encoder.resolve_query_set(&query_set, 0..2, &resolve_buf, 0);
```

4. Read back and convert to nanoseconds:

```rust
// After mapping the resolve buffer:
let timestamps: &[u64; 2] = /* read from mapped buffer */;
let period = queue.get_timestamp_period(); // nanoseconds per tick
let gpu_time_ns = (timestamps[1] - timestamps[0]) as f64 * period as f64;
```

**Note from wgpu docs:** "Since commands within a command recorder may be reordered, there is no
strict guarantee that timestamps are taken after all commands recorded so far." Timestamps at pass
boundaries (beginning/end) are well-defined.

### Standalone Timestamp Writes

For timing work outside render/compute passes (e.g., buffer copies):

```rust
encoder.write_timestamp(&query_set, 0);
// ... some commands ...
encoder.write_timestamp(&query_set, 1);
```

### OpenGL Timer Queries (via raw backend)

If targeting OpenGL (e.g., for older hardware or Linux without Vulkan), use `GL_TIME_ELAPSED` or
`GL_TIMESTAMP` queries. With wgpu, this requires dropping to the HAL layer via
`CommandEncoder::as_hal_mut`, which is an advanced escape hatch. Prefer the WebGPU timestamp query
API when possible.

### Frame Time Tracking Pattern

For continuous frame-time monitoring (not micro-benchmarks):

```rust
struct FrameTimer {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    period_ns: f32,
    history: VecDeque<f64>, // rolling window of frame times
}

impl FrameTimer {
    fn report(&self) -> FrameStats {
        let sorted: Vec<f64> = /* sort history */;
        FrameStats {
            avg_ms: sorted.iter().sum::<f64>() / sorted.len() as f64 / 1e6,
            p99_ms: sorted[(sorted.len() as f64 * 0.99) as usize] / 1e6,
            max_ms: *sorted.last().unwrap() / 1e6,
        }
    }
}
```

---

## 4. Input-to-Display Latency

End-to-end latency from keypress to pixel change is a key terminal quality metric. This is harder to
measure than throughput because it spans the entire pipeline.

### Approaches

**1. External measurement (ground truth):**

- [Typometer](https://github.com/pavelfatin/typometer): Java tool that uses screen capture to
  measure keystroke-to-display latency. Works with any terminal. Records at 1000Hz, detects pixel
  changes after synthetic keypresses.
- [Is It Snappy](https://isitsnappy.com/): High-speed camera approach. Requires 240Hz+ camera
  pointed at the screen while typing.

**2. Software instrumentation:** Insert timestamps at pipeline boundaries and report the delta:

```rust
struct LatencyTracer {
    input_timestamp: Option<Instant>,
    parse_done: Option<Instant>,
    layout_done: Option<Instant>,
    render_submitted: Option<Instant>,
    frame_presented: Option<Instant>, // from GPU fence/callback
}

impl LatencyTracer {
    fn report(&self) -> LatencyBreakdown {
        LatencyBreakdown {
            input_to_parse: self.parse_done.unwrap() - self.input_timestamp.unwrap(),
            parse_to_layout: self.layout_done.unwrap() - self.parse_done.unwrap(),
            layout_to_submit: self.render_submitted.unwrap() - self.layout_done.unwrap(),
            total_cpu: self.render_submitted.unwrap() - self.input_timestamp.unwrap(),
            // GPU time from timestamp queries (see section 3)
        }
    }
}
```

**3. Self-echo latency test:** Send a character to the PTY, time how long until it appears in the
grid buffer:

```rust
fn measure_echo_latency(pty: &mut Pty, grid: &Grid) -> Duration {
    let before = grid.version(); // or a content hash
    let start = Instant::now();
    pty.write(b"x");
    loop {
        poll_pty(pty, grid);
        if grid.version() != before {
            return start.elapsed();
        }
    }
}
```

### Latency Budget

Typical targets for a responsive terminal:

- Total input-to-display: < 16ms (60fps) or < 8ms (120fps)
- PTY read + parse: < 1ms
- Grid update: < 1ms
- Diff computation: < 1ms
- GPU render + present: < 5ms
- Remaining budget for vsync alignment

---

## 5. Memory Profiling

### 5.1 DHAT (dhat-rs crate)

[dhat](https://docs.rs/dhat) is a Rust crate for heap profiling that works on all platforms. It
wraps the global allocator and records every allocation.

**Setup:**

```toml
[features]
dhat-heap = []

[dependencies]
dhat = { version = "0.3", optional = true }

[profile.release]
debug = 1  # needed for readable backtraces
```

```rust
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    // ... application code ...
}
// On drop, writes dhat-heap.json
// View at: https://nnethercote.github.io/dh_view/dh_view.html
```

Run: `cargo run --release --features dhat-heap`

**Heap usage testing (for CI):**

```rust
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn test_grid_allocation() {
    let _profiler = dhat::Profiler::builder().testing().build();

    let grid = Grid::new(24, 80);
    let stats = dhat::HeapStats::get();

    // Verify cell storage is a single contiguous allocation
    // 24 * 80 cells * size_of::<Cell>() bytes
    let expected_bytes = 24 * 80 * std::mem::size_of::<Cell>();
    dhat::assert_eq!(stats.total_blocks, 1);
    assert!(stats.total_bytes <= expected_bytes + 256); // small overhead OK
}
```

Important: each heap usage test must be in its own integration test file (separate process), or use
`--test-threads=1`, because dhat uses global state.

### 5.2 Heaptrack

[heaptrack](https://github.com/KDE/heaptrack) is a Linux heap profiler that intercepts malloc/free
via LD_PRELOAD. Zero source code changes needed.

```bash
# Profile
heaptrack ./target/release/my-terminal

# Analyze
heaptrack_gui heaptrack.my-terminal.*.zst
```

Shows: peak memory, allocation hotspots, memory leaks, allocation frequency over time, flamegraph of
allocation call stacks.

Not available on macOS. On macOS, use Instruments (Allocations template) or
`cargo instruments -t Allocations`.

### 5.3 Measuring Bytes Per Cell

This is a key metric for terminal grids. Track it with a compile-time assertion and a runtime check:

```rust
// Compile-time size check
const _: () = assert!(
    std::mem::size_of::<Cell>() <= 16,
    "Cell size exceeds 16 bytes budget"
);

// Runtime allocation test
#[test]
fn bytes_per_cell_budget() {
    let cell_size = std::mem::size_of::<Cell>();
    let grid_cells = 200 * 60; // large terminal
    let grid_bytes = cell_size * grid_cells;

    println!("Cell size: {} bytes", cell_size);
    println!("Grid 200x60: {} KB", grid_bytes / 1024);

    assert!(cell_size <= 16, "Cell too large: {} bytes", cell_size);
    // 200x60 grid at 16 bytes/cell = 187.5 KB. Comfortable.
}
```

Use `#[repr(C)]` or `#[repr(packed)]` cautiously; prefer `#[repr(Rust)]` and let the compiler
optimize layout. Use `std::mem::size_of::<T>()` and `std::mem::align_of::<T>()` to verify.

### 5.4 Divan's AllocProfiler

Divan has built-in allocation profiling. Add to your benchmark harness:

```rust
#[global_allocator]
static ALLOC: divan::AllocProfiler = divan::AllocProfiler::system();

fn main() {
    divan::main();
}

#[divan::bench]
fn grid_operations() -> Grid {
    let mut grid = Grid::new(24, 80);
    grid.clear(Cell::default());
    grid
}
```

Output shows allocation count and bytes alongside timing, per benchmark.

---

## 6. Flame Graphs

### cargo-flamegraph

[cargo-flamegraph](https://github.com/flamegraph-rs/flamegraph) generates flamegraph SVGs from Rust
projects. Built on [inferno](https://github.com/jonhoo/inferno).

**Install:**

```bash
cargo install flamegraph
```

**Platform requirements:**

- Linux: `perf` (install `linux-tools-common linux-tools-generic`)
- macOS: `xctrace` (comes with Xcode)
- Windows: works out of the box via [blondie](https://github.com/nico-abram/blondie), or dtrace

**Usage:**

```bash
# Profile the default binary
cargo flamegraph

# Profile a specific benchmark
cargo flamegraph --bench grid_bench -- --bench

# Profile with custom perf events
cargo flamegraph -c "record -e cache-misses -c 100 --call-graph lbr -g"

# Profile an example
cargo flamegraph --example render_demo
```

**Important config for readable output:**

```toml
# Cargo.toml
[profile.release]
debug = true  # keep symbols

[profile.bench]
debug = true
```

**Linux with lld (Rust 1.90+):** Add `--no-rosegment` linker flag or perf can't read stack traces:

```toml
# .cargo/config.toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-Clink-arg=-Wl,--no-rosegment"]
```

**Reading flamegraphs:**

- Width = proportion of total samples containing that function
- Y-axis = call stack depth
- X-axis ordering is alphabetical, NOT time-based
- Color is random (not meaningful)
- Look for wide boxes near the top: these are the most expensive leaf functions

### samply (alternative)

[samply](https://github.com/mstange/samply) is another Rust profiler that integrates with Firefox's
Profiler web UI. Better macOS support than cargo-flamegraph, and provides an interactive timeline
view rather than a static SVG.

```bash
cargo install samply
samply record ./target/release/my-binary
# Opens Firefox Profiler automatically
```

---

## 7. WASM Profiling

### Chrome DevTools Performance Tab

When building for `wasm32-unknown-unknown`, Chrome DevTools can profile WASM execution.

**Build with debug names:**

```toml
# Cargo.toml
[profile.release]
debug = "line-tables-only"  # or debug = true for full info
```

Or use `wasm-pack build --profiling` which enables debug symbols without disabling optimizations.

DevTools Performance tab will show Rust function names in the flame chart if the WASM binary
includes the "name" custom section. Without debug symbols, you see opaque `wasm-function[123]`
labels.

Caveat: inlined functions won't appear. Rust/LLVM inline aggressively, so the call tree may look
flattened.

### console.time / console.timeEnd

Use `web-sys` to add manual timing points:

```rust
use web_sys::console;

pub fn render_frame(grid: &Grid) {
    console::time_with_label("render_frame");

    console::time_with_label("diff");
    let changes = compute_diff(grid);
    console::time_end_with_label("diff");

    console::time_with_label("upload");
    upload_to_gpu(&changes);
    console::time_end_with_label("upload");

    console::time_with_label("draw");
    draw_pass();
    console::time_end_with_label("draw");

    console::time_end_with_label("render_frame");
}
```

These appear in both the console and the DevTools Performance timeline/waterfall.

### performance.now()

For programmatic measurements via `web-sys`:

```rust
fn now() -> f64 {
    web_sys::window()
        .expect("should have a Window")
        .performance()
        .expect("should have a Performance")
        .now()
}

pub fn measure<F: FnOnce() -> R, R>(label: &str, f: F) -> R {
    let start = now();
    let result = f();
    let elapsed = now() - start;
    web_sys::console::log_2(
        &format!("{}: {:.2}ms", label, elapsed).into(),
        &wasm_bindgen::JsValue::UNDEFINED,
    );
    result
}
```

### Native Benchmarks for WASM Code

The most effective approach: write `#[bench]` or `#[divan::bench]` functions for your core logic,
run them natively. WASM-specific overhead (JS interop, DOM) is a thin layer; the core
grid/diff/layout logic is platform-independent. Profile the native build with cargo-flamegraph, then
verify the WASM build matches expectations using DevTools.

---

## 8. Prior Art: Terminal Emulator Benchmarks

### vtebench (Alacritty)

[vtebench](https://github.com/alacritty/vtebench) measures terminal PTY read performance. It
generates VT escape sequence payloads and measures how fast a terminal can consume them.

**What it measures:** Raw PTY read speed only. NOT frame rate, latency, or rendering performance.

**How it works:**

- Benchmarks are defined as directories with a `benchmark` executable and an optional `setup` script
- The benchmark's stdout is used as the payload sent to the terminal via PTY
- Results can be plotted with gnuplot

```bash
git clone https://github.com/alacritty/vtebench
cd vtebench
cargo run --release           # runs all benchmarks
cargo run --release -- --dat results.dat  # machine-readable output
./gnuplot/summary.sh results.dat output.svg  # plot
```

The vtebench README explicitly warns: "This benchmark is not sufficient to get a general
understanding of the performance of a terminal emulator." It only tests one dimension.

### Alacritty's Internal Benchmarks

Alacritty uses Criterion internally for benchmarking grid operations, VT parsing, and the renderer.
Their benchmark files live under `alacritty_terminal/benches/` and cover:

- VT parser throughput (bytes/second of escape sequence processing)
- Grid scrolling and line operations
- Cell iteration patterns

### termbench

[termbench](https://github.com/gizmo98/termbench) is a simpler terminal benchmark that measures raw
text output speed by writing large amounts of text and timing how fast the terminal displays it.
Less sophisticated than vtebench but easier to run.

### Key Lessons from Terminal Benchmarks

1. **Separate PTY throughput from render throughput.** A terminal can be fast at reading PTY data
   but slow at rendering, or vice versa.
2. **Measure at multiple grid sizes.** 80x24 vs 200x60 can show different bottlenecks (memory
   bandwidth vs computation).
3. **Test with realistic workloads:** scrolling, colored output, wide characters, cursor movement,
   alternate screen buffer.
4. **VT parser throughput** is often the first bottleneck. Benchmark it in isolation.

---

## 9. CI Regression Testing

### Option A: github-action-benchmark

[github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark) is a GitHub
Action that parses benchmark output from `cargo bench` (Criterion format), stores results in
`gh-pages` branch, and alerts on regressions.

```yaml
name: Benchmarks
on:
  push:
    branches: [main]

jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Run benchmarks
        run: cargo bench --bench grid_bench -- --output-format bencher | tee output.txt

      - name: Download previous benchmark data
        uses: actions/cache@v4
        with:
          path: ./cache
          key: ${{ runner.os }}-benchmark

      - name: Store benchmark result
        uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: 'cargo'
          output-file-path: output.txt
          external-data-json-path: ./cache/benchmark-data.json
          fail-on-alert: true
          alert-threshold: '150%'
          github-token: ${{ secrets.GITHUB_TOKEN }}
          comment-on-alert: true
```

Default alert threshold is 200% (current is 2x worse than previous). Set `alert-threshold: '120%'`
for tighter regression detection.

Caveat: shared CI runners have ~30% variance. Results will be noisy. Consider:

- Running multiple iterations and averaging
- Using `fail-threshold` separate from `alert-threshold`
- Using self-hosted runners for stability

### Option B: Bencher

[Bencher](https://bencher.dev) is a continuous benchmarking platform with optional bare-metal
runners for <2% variance. Supports Criterion, Divan, and custom output formats.

```yaml
name: Continuous Benchmarking
on:
  push:
    branches: [main]

jobs:
  bench:
    runs-on: ubuntu-latest
    env:
      BENCHER_PROJECT: my-terminal
      BENCHER_API_TOKEN: ${{ secrets.BENCHER_API_TOKEN }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: bencherdev/bencher@main
      - run: bencher run "cargo bench --bench grid_bench"
```

Bencher provides:

- Historical tracking with charts
- Statistical regression detection (configurable thresholds)
- PR comments showing benchmark comparisons
- Support for self-hosted deployments

### Option C: Manual Comparison Script

For simpler setups, use `critcmp` with Criterion:

```bash
# Save baseline
cargo bench --bench grid_bench -- --save-baseline main

# On the branch
cargo bench --bench grid_bench -- --save-baseline pr

# Compare
cargo install critcmp
critcmp main pr
```

Or with Divan, capture output and diff:

```bash
# baseline
cargo bench --bench grid_bench > baseline.txt

# PR
cargo bench --bench grid_bench > pr.txt

# manual diff (Divan doesn't have critcmp equivalent yet)
diff baseline.txt pr.txt
```

---

## 10. Concrete Setup and Code Examples

### Cargo.toml

```toml
[package]
name = "rg"
version = "0.1.0"
edition = "2021"

[dependencies]
# ... your dependencies ...

[dev-dependencies]
divan = "0.1"
fastrand = "2"

[features]
dhat-heap = ["dep:dhat"]

[dependencies.dhat]
version = "0.3"
optional = true

# Benchmark binary (one per file in benches/)
[[bench]]
name = "grid"
harness = false

[[bench]]
name = "parser"
harness = false

[[bench]]
name = "renderer"
harness = false

# Keep symbols in release builds for flamegraphs and DHAT
[profile.release]
debug = 1

[profile.bench]
debug = 1
```

### benches/grid.rs

```rust
use rg::{Grid, Cell, Style};

fn main() {
    divan::main();
}

// --- Cell Write Throughput ---

#[divan::bench(consts = [80, 132, 200])]
fn write_cells_sequential<const COLS: usize>(bencher: divan::Bencher) {
    let rows = 24;
    let mut grid = Grid::new(rows, COLS);
    bencher
        .counter(divan::counter::ItemsCount::new(rows * COLS))
        .bench(|| {
            for row in 0..rows {
                for col in 0..COLS {
                    grid.write_cell(row, col, Cell::new('A', Style::default()));
                }
            }
        });
}

// --- Grid Clear ---

#[divan::bench(args = [80*24, 132*50, 200*60])]
fn grid_clear(bencher: divan::Bencher, total_cells: usize) {
    let cols = 80;
    let rows = total_cells / cols;
    let mut grid = Grid::new(rows, cols);
    bencher
        .counter(divan::counter::ItemsCount::new(total_cells))
        .bench(|| {
            grid.clear(Cell::default());
        });
}

// --- Diff ---

mod diff {
    use super::*;

    #[divan::bench]
    fn sparse_5pct(bencher: divan::Bencher) {
        let (rows, cols) = (50, 200);
        let old = Grid::filled(rows, cols, Cell::default());
        let mut new = old.clone();
        let mut rng = fastrand::Rng::with_seed(42);
        for _ in 0..(rows * cols / 20) {
            new.write_cell(
                rng.usize(..rows),
                rng.usize(..cols),
                Cell::new('X', Style::bold()),
            );
        }
        bencher.bench(|| rg::diff(&old, &new));
    }

    #[divan::bench]
    fn full_repaint(bencher: divan::Bencher) {
        let old = Grid::filled(50, 200, Cell::default());
        let new = Grid::filled(50, 200, Cell::new('X', Style::bold()));
        bencher.bench(|| rg::diff(&old, &new));
    }

    #[divan::bench]
    fn no_changes(bencher: divan::Bencher) {
        let grid = Grid::filled(50, 200, Cell::default());
        bencher.bench(|| rg::diff(&grid, &grid));
    }
}
```

### benches/parser.rs

```rust
fn main() {
    divan::main();
}

#[divan::bench]
fn parse_ascii_stream(bencher: divan::Bencher) {
    // Simulate a large stream of ASCII text with newlines
    let data: Vec<u8> = (0..100_000)
        .map(|i| if i % 80 == 79 { b'\n' } else { b'A' + (i % 26) as u8 })
        .collect();

    bencher
        .counter(divan::counter::BytesCount::new(data.len()))
        .bench(|| {
            let mut parser = rg::Parser::new();
            parser.advance(&data);
        });
}

#[divan::bench]
fn parse_sgr_colors(bencher: divan::Bencher) {
    // Dense SGR color escape sequences
    let mut data = Vec::new();
    for i in 0..10_000 {
        data.extend_from_slice(format!("\x1b[38;5;{}mX", i % 256).as_bytes());
    }

    bencher
        .counter(divan::counter::BytesCount::new(data.len()))
        .bench(|| {
            let mut parser = rg::Parser::new();
            parser.advance(&data);
        });
}

#[divan::bench]
fn parse_unicode_mixed(bencher: divan::Bencher) {
    let text = "Hello 世界 🌍 Ñoño\n".repeat(5000);
    let data = text.as_bytes();

    bencher
        .counter(divan::counter::BytesCount::new(data.len()))
        .bench(|| {
            let mut parser = rg::Parser::new();
            parser.advance(data);
        });
}
```

### benches/renderer.rs

```rust
fn main() {
    divan::main();
}

// Renderer benchmarks typically need GPU setup.
// For pure CPU benchmarks (vertex generation, instance buffer packing):

#[divan::bench(args = [(80, 24), (200, 60)])]
fn build_instance_buffer(bencher: divan::Bencher, (cols, rows): (usize, usize)) {
    let grid = rg::test_helpers::make_grid(rows, cols);
    let mut instances = Vec::with_capacity(rows * cols);

    bencher
        .counter(divan::counter::ItemsCount::new(rows * cols))
        .bench(|| {
            instances.clear();
            rg::renderer::build_instances(&grid, &mut instances);
        });
}

#[divan::bench]
fn atlas_lookup_ascii(bencher: divan::Bencher) {
    let atlas = rg::test_helpers::make_warmed_atlas();
    bencher.bench(|| {
        for c in 0x20u8..0x7F {
            divan::black_box(atlas.get_glyph(c as char, 14.0));
        }
    });
}
```

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark file
cargo bench --bench grid

# Filter to specific benchmark function
cargo bench --bench grid -- 'diff'

# With allocation profiling (requires AllocProfiler in harness)
cargo bench --bench grid

# Generate flamegraph for a benchmark
cargo flamegraph --bench grid -- --bench 'write_cells'

# Memory profiling with DHAT
cargo run --release --features dhat-heap

# DHAT in tests
cargo test --release --features dhat-heap -- test_grid_allocation
```

---

## Sources

- [Criterion.rs](https://github.com/bheisler/criterion.rs) / [docs](https://docs.rs/criterion) --
  statistics-driven benchmarking, HTML reports, baseline comparison
- [Divan](https://github.com/nvzqz/divan) / [announcement](https://nikolaivazquez.com/blog/divan/)
  -- simple API, allocation profiling, generic benchmarks, thread contention
- [dhat crate](https://docs.rs/dhat) -- heap profiling and allocation testing for Rust
- [cargo-flamegraph](https://github.com/flamegraph-rs/flamegraph) -- flamegraph SVG generation,
  perf/dtrace/xctrace backends
- [wgpu CommandEncoder::write_timestamp](https://docs.rs/wgpu/latest/wgpu/struct.CommandEncoder.html)
  -- GPU timestamp queries
- [wgpu RenderPassTimestampWrites](https://docs.rs/wgpu/latest/wgpu/struct.RenderPassTimestampWrites.html)
  -- per-pass GPU timing
- [vtebench](https://github.com/alacritty/vtebench) -- PTY read performance benchmarking for
  terminals
- [github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark) -- GitHub
  Action for CI regression detection
- [Bencher](https://bencher.dev) / [repo](https://github.com/bencherdev/bencher) -- continuous
  benchmarking platform with bare-metal runners
- [Rust WASM Book: Time Profiling](https://rustwasm.github.io/book/reference/time-profiling.html) --
  WASM profiling with DevTools, console.time, performance.now
