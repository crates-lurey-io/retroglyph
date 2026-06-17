# Testing Strategies for a Rust Terminal/Grid Rendering Library

## Summary

A terminal/grid rendering library benefits from a layered testing strategy: snapshot tests for
output correctness, property-based tests for invariants, fuzzing for parser robustness, benchmarks
for performance regression detection, and a headless test backend to enable all of the above without
a real terminal. The patterns established by ratatui (TestBackend, `assert_buffer_lines`,
Buffer-centric widget tests) and the broader Rust ecosystem (insta, proptest, cargo-fuzz,
criterion/divan) provide a proven foundation.

---

## 1. Snapshot Testing with insta

### Approach

Serialize a `Buffer` or `Cell` grid to a human-readable text representation, then compare against
golden `.snap` files managed by the [insta](https://docs.rs/insta/) crate.

### Implementation

```rust
use insta::assert_snapshot;

#[test]
fn test_grid_renders_box() {
    let mut grid = Grid::new(10, 5);
    draw_box(&mut grid, Rect::new(0, 0, 10, 5));
    // Serialize grid to a text representation
    assert_snapshot!(grid.to_text_repr());
}
```

The serialization function should produce stable, readable output:

```rust
impl Grid {
    /// Render grid content as a line-per-row string for snapshot comparison.
    fn to_text_repr(&self) -> String {
        let mut out = String::new();
        for row in 0..self.height {
            out.push('"');
            for col in 0..self.width {
                out.push_str(self.cell(col, row).symbol());
            }
            out.push_str("\"\n");
        }
        out
    }
}
```

### insta Workflow

1. Run tests: `cargo test` (new snapshots written to `.snap.new` files)
2. Review: `cargo insta review` (interactive accept/reject)
3. Commit accepted `.snap` files to version control

Key `INSTA_UPDATE` modes:

- `auto` (default): `new` locally, `no` in CI
- `no`: CI mode, fails on any mismatch without writing files
- `always`: auto-accept all changes (useful for bulk updates)

### When to Use Snapshots

- Widget rendering output (the entire grid after rendering a component)
- ANSI escape sequence generation (comparing raw output strings)
- Diff algorithm output (what cells changed between two buffers)
- Debug formatting of complex structures

### Recommended insta Configuration

```toml
# Cargo.toml
[dev-dependencies]
insta = { version = "1", features = ["yaml", "redactions"] }

# Faster test runs
[profile.dev.package.insta]
opt-level = 3
[profile.dev.package.similar]
opt-level = 3
```

[Source: insta docs](https://docs.rs/insta/latest/insta/)

---

## 2. Visual Regression Testing

### Approach

Render the grid to an image (PNG) via a headless backend, then compare pixel-by-pixel against
reference images. This catches rendering issues that text-based snapshots miss: font metrics, color
blending, glyph positioning.

### Implementation Strategy

```rust
/// Headless backend that renders cells to a pixel buffer.
struct ImageBackend {
    font: Font,
    pixel_buffer: Vec<u8>,
    width_px: u32,
    height_px: u32,
}

impl ImageBackend {
    fn render_to_png(&self) -> Vec<u8> {
        // Rasterize each cell using the font, write to PNG
    }
}

#[test]
fn test_visual_output() {
    let grid = render_demo_screen();
    let backend = ImageBackend::new(grid.width(), grid.height());
    backend.draw(grid.diff_iter());
    let png = backend.render_to_png();

    // Compare against golden image with tolerance
    let reference = std::fs::read("tests/golden/demo_screen.png").unwrap();
    assert_images_equal(&png, &reference, tolerance: 0.01);
}
```

### Image Comparison Options

- **Pixel-diff with tolerance**: allow small per-pixel color variance (handles font rendering
  differences across platforms)
- **Perceptual hash**: compare structural similarity rather than exact pixels
- **Crate options**: `image` for PNG encoding/decoding, `pixelmatch` pattern for diffing

### Practical Considerations

- Pin font and font size in tests to avoid cross-platform variance
- Store golden images in `tests/golden/` and track in version control (use Git LFS for large sets)
- Set a pixel tolerance threshold (e.g., 1% RMSE) to handle anti-aliasing differences
- Run visual tests only on a single reference platform in CI; skip on others via `#[cfg]` or feature
  flags

---

## 3. Property-Based Testing with proptest

### Approach

Use [proptest](https://proptest-rs.github.io/proptest/) to generate random inputs and verify
invariants hold for all of them. When a failure is found, proptest automatically shrinks to a
minimal failing case.

### Key Invariants for a Grid Library

1. **Grid dimensions**: `grid.width() * grid.height() == grid.cells().len()`
2. **Cell writes within bounds**: writing to any valid `(x, y)` succeeds; writing outside panics or
   returns `None`
3. **Wide character spacers**: every wide (2-cell) character at position `(x, y)` has a
   spacer/continuation cell at `(x+1, y)`
4. **Diff symmetry**: `diff(a, b)` applied to `a` produces `b`
5. **Merge idempotency**: `a.merge(&b); assert!(a.diff(&b).is_empty())`
6. **Resize preserves content**: resizing then reading in-bounds cells returns original values

### Example: Grid Dimension Invariant

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn grid_dimensions_consistent(
        width in 1u16..500,
        height in 1u16..500,
    ) {
        let grid = Grid::new(width, height);
        prop_assert_eq!(grid.cells().len(), (width as usize) * (height as usize));
        prop_assert_eq!(grid.width(), width);
        prop_assert_eq!(grid.height(), height);
    }
}
```

### Example: Wide Character Spacer Invariant

```rust
proptest! {
    #[test]
    fn wide_chars_always_have_spacer(
        width in 4u16..100,
        height in 1u16..50,
        x in 0u16..98,  // leave room for spacer
        y in 0u16..49,
    ) {
        prop_assume!(x < width - 1 && y < height);
        let mut grid = Grid::new(width, height);
        grid.set_wide_char(x, y, "漢");

        // Primary cell should contain the character
        prop_assert_eq!(grid.cell(x, y).symbol(), "漢");
        prop_assert_eq!(grid.cell(x, y).cell_width(), 2);

        // Next cell should be a spacer/continuation
        prop_assert_eq!(grid.cell(x + 1, y).symbol(), " ");
    }
}
```

### Example: Out-of-Bounds Write Safety

```rust
proptest! {
    #[test]
    fn cell_access_returns_none_for_oob(
        width in 1u16..100,
        height in 1u16..100,
        x in 0u16..200,
        y in 0u16..200,
    ) {
        let grid = Grid::new(width, height);
        let result = grid.cell(Position::new(x, y));
        if x < width && y < height {
            prop_assert!(result.is_some());
        } else {
            prop_assert!(result.is_none());
        }
    }
}
```

### Configuration

```toml
[dev-dependencies]
proptest = "1"
```

Proptest stores failure regressions in `proptest-regressions/` files. Commit these to version
control so known failures are replayed on every test run.

[Source: Proptest Book](https://proptest-rs.github.io/proptest/intro.html)

---

## 4. Fuzzing Input Parsing

### Why Fuzz a Terminal Library

ANSI escape sequence parsers and configuration string parsers handle untrusted, complex input.
Fuzzing finds panics, buffer overflows, infinite loops, and logic errors that hand-written tests
miss.

### Setup with cargo-fuzz (libFuzzer)

```bash
# Requires nightly Rust
rustup default nightly
cargo install cargo-fuzz
cargo fuzz init
cargo fuzz add parse_ansi
```

### Fuzz Target: ANSI Escape Sequence Parsing

```rust
// fuzz/fuzz_targets/parse_ansi.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Parser should handle arbitrary byte sequences without panicking
    let mut parser = rg::AnsiParser::new();
    for &byte in data {
        let _ = parser.advance(byte);
    }
    // Verify parser is in a consistent state
    let _ = parser.flush();
});
```

### Structure-Aware Fuzzing

For more targeted coverage, generate structured inputs using the `Arbitrary` trait:

```rust
use libfuzzer_sys::arbitrary::{self, Arbitrary};

#[derive(Debug, Arbitrary)]
enum AnsiSequence {
    Csi { params: Vec<u16>, intermediate: Vec<u8>, final_byte: u8 },
    Osc { params: Vec<Vec<u8>> },
    Escape { intermediate: Vec<u8>, final_byte: u8 },
    Print(char),
    Control(u8),
}

fuzz_target!(|sequences: Vec<AnsiSequence>| {
    let bytes = sequences_to_bytes(&sequences);
    let mut parser = rg::AnsiParser::new();
    for &byte in &bytes {
        let _ = parser.advance(byte);
    }
});
```

### Fuzz Target: Configuration String Parsing

```rust
// fuzz/fuzz_targets/parse_config.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Should not panic on any input
        let _ = rg::Config::parse(s);
    }
});
```

### Running Fuzz Tests

```bash
# Run indefinitely until crash found
cargo fuzz run parse_ansi

# Run for 5 minutes (CI smoke test)
cargo fuzz run parse_ansi -- -max_total_time=300

# Check coverage
cargo fuzz coverage parse_ansi
```

### Alternative: AFL.rs

```bash
cargo install afl
# AFL uses different instrumentation, can find different bugs
cargo afl build
cargo afl fuzz -i seeds/ -o out/ target/debug/fuzz_parse_ansi
```

### CI Integration for Fuzzing

Run fuzz targets as smoke tests (5-minute runs) on pushes to main:

```yaml
# .github/workflows/fuzz.yml
name: Fuzz
on:
  push:
    branches: [main]
jobs:
  fuzz:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target: [parse_ansi, parse_config]
    steps:
      - uses: actions/checkout@v4
      - run: rustup toolchain install nightly && rustup default nightly
      - uses: actions/cache@v4
        with:
          path: ${{ runner.tool_cache }}/cargo-fuzz
          key: cargo-fuzz-0.12.0
      - run: |
          echo "${{ runner.tool_cache }}/cargo-fuzz/bin" >> $GITHUB_PATH
          cargo install --root "${{ runner.tool_cache }}/cargo-fuzz" --version 0.12.0 cargo-fuzz --locked
      - run: cargo fuzz build ${{ matrix.target }}
      - run: cargo fuzz run ${{ matrix.target }} -- -max_total_time=300
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: fuzz-artifacts-${{ matrix.target }}
          path: fuzz/artifacts
```

[Source: Rust Fuzz Book](https://rust-fuzz.github.io/book/)

---

## 5. Headless/Test Backend as Test Harness

### The Pattern

A test backend is an in-memory implementation of the `Backend` trait that stores rendered output in
a `Buffer` rather than writing to a real terminal. This makes tests fast, deterministic, and
CI-friendly.

### Ratatui's TestBackend (reference implementation)

From ratatui's `ratatui-core/src/backend/test.rs`:

```rust
pub struct TestBackend {
    buffer: Buffer,
    scrollback: Buffer,
    cursor: bool,
    pos: (u16, u16),
}

impl TestBackend {
    pub fn new(width: u16, height: u16) -> Self { /* ... */ }

    /// Create from initial screen content
    pub fn with_lines<Lines>(lines: Lines) -> Self { /* ... */ }

    /// Read the current buffer state
    pub const fn buffer(&self) -> &Buffer { &self.buffer }

    /// Assert buffer matches expected lines
    pub fn assert_buffer_lines<Lines>(&self, expected: Lines) { /* ... */ }

    /// Assert cursor position
    pub fn assert_cursor_position<P: Into<Position>>(&mut self, position: P) { /* ... */ }
}

impl Backend for TestBackend {
    type Error = core::convert::Infallible;  // never fails

    fn draw<'a, I>(&mut self, content: I) -> Result<()>
    where I: Iterator<Item = (u16, u16, &'a Cell)> {
        for (x, y, c) in content {
            self.buffer[(x, y)] = c.clone();
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        self.buffer.reset();
        Ok(())
    }

    fn size(&self) -> Result<Size> {
        Ok(self.buffer.area.as_size())
    }
    // ... cursor, flush, append_lines, scroll_region_up/down
}
```

### Key Design Decisions

1. **Error type is `Infallible`**: test backend operations never fail, simplifying test code
2. **Buffer is inspectable**: direct access to read cell content after rendering
3. **Assertion helpers built in**: `assert_buffer_lines`, `assert_cursor_position`,
   `assert_scrollback_lines` reduce test boilerplate
4. **Scrollback tracking**: captures lines that scroll off-screen, testing scroll behavior
5. **Serde support**: `#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]` enables
   snapshot serialization

### Recommended Test Backend API for a New Library

```rust
pub struct TestBackend {
    grid: Grid,
    cursor_visible: bool,
    cursor_pos: Position,
}

impl TestBackend {
    pub fn new(width: u16, height: u16) -> Self;
    pub fn with_lines(lines: impl IntoIterator<Item = impl Into<Line>>) -> Self;
    pub fn grid(&self) -> &Grid;
    pub fn assert_lines(&self, expected: impl IntoIterator<Item = impl AsRef<str>>);
    pub fn assert_cell(&self, pos: Position, expected_symbol: &str);
}
```

[Source: ratatui TestBackend](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend/test.rs)

---

## 6. Integration Testing Across Backends

### The Problem

A terminal library typically supports multiple backends (crossterm, termion, termwiz, etc.). Each
backend may have subtle behavioral differences. Testing against all backends ensures correctness is
not backend-specific.

### Ratatui's Approach

From ratatui's CI configuration, backend testing runs as a matrix:

```yaml
test-backends:
  name: Test ${{matrix.backend}} on ${{ matrix.os }}
  runs-on: ${{ matrix.os }}
  strategy:
    fail-fast: false
    matrix:
      os: [ubuntu-latest, windows-latest, macos-latest]
      backend: [crossterm, termion, termina, termwiz]
      exclude:
        - os: windows-latest
          backend: termion # termion doesn't support Windows
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@master
      with: { toolchain: stable }
    - run: cargo xtask test-backend ${{ matrix.backend }}
```

### Pattern: Backend-Agnostic Test Suite

Write tests against the `Backend` trait, then run them with each backend:

```rust
fn run_backend_tests<B: Backend>(mut backend: B) {
    backend.clear().unwrap();
    let size = backend.size().unwrap();
    assert!(size.width > 0);
    assert!(size.height > 0);

    backend.hide_cursor().unwrap();
    backend.show_cursor().unwrap();

    let cell = Cell::new("X");
    backend.draw([(0, 0, &cell)].into_iter()).unwrap();
    backend.flush().unwrap();
}

#[test]
fn test_with_test_backend() {
    run_backend_tests(TestBackend::new(80, 24));
}

#[test]
#[cfg(feature = "crossterm")]
fn test_with_crossterm() {
    // Only runs when a real terminal is available (skip in CI headless)
    if !atty::is(atty::Stream::Stdout) { return; }
    run_backend_tests(CrosstermBackend::new(std::io::stdout()));
}
```

### Backend Feature Flags

Use Cargo features to conditionally compile backend-specific tests:

```toml
[features]
crossterm = ["dep:crossterm"]
termion = ["dep:termion"]
termwiz = ["dep:termwiz"]
test-backend = []  # always available
```

[Source: ratatui CI](https://github.com/ratatui/ratatui/blob/main/.github/workflows/ci.yml)

---

## 7. Benchmark Testing with criterion/divan

### What to Benchmark

- **Cells per second**: throughput of writing individual cells to the grid
- **Full grid render time**: time to render an entire screen (e.g., 80x24, 200x50)
- **Diff computation time**: time to compute the diff between two buffers
- **ANSI parsing throughput**: bytes/second through the escape sequence parser

### Option A: criterion

[criterion](https://docs.rs/criterion/) provides statistical analysis with confidence intervals and
detects regressions.

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "grid_benchmarks"
harness = false
```

```rust
// benches/grid_benchmarks.rs
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_cell_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("cell_write");
    for size in [80*24, 200*50, 400*100] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            format!("{size}_cells"),
            &size,
            |b, &size| {
                let width = (size as f64).sqrt() as u16;
                let height = width;
                b.iter(|| {
                    let mut grid = Grid::new(width, height);
                    for y in 0..height {
                        for x in 0..width {
                            grid.cell_mut(x, y).set_symbol("A");
                        }
                    }
                    criterion::black_box(&grid);
                });
            },
        );
    }
    group.finish();
}

fn bench_diff_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff");
    let prev = Grid::filled(80, 24, Cell::new("a"));
    let mut next = Grid::filled(80, 24, Cell::new("a"));
    // Change 10% of cells
    for i in 0..192 {
        let x = (i * 7) % 80;
        let y = (i * 13) % 24;
        next.cell_mut(x as u16, y as u16).set_symbol("b");
    }
    group.bench_function("80x24_10pct_changed", |b| {
        b.iter(|| {
            let diff: Vec<_> = prev.diff(&next).collect();
            criterion::black_box(diff);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_cell_write, bench_diff_computation);
criterion_main!(benches);
```

### Option B: divan

[divan](https://docs.rs/divan/) uses `#[divan::bench]` attributes, producing cleaner code with less
boilerplate. It also has built-in allocation profiling.

```toml
[dev-dependencies]
divan = "0.1"

[[bench]]
name = "grid_benchmarks"
harness = false
```

```rust
// benches/grid_benchmarks.rs
fn main() {
    divan::main();
}

#[divan::bench(args = [80*24, 200*50, 400*100])]
fn cell_write(n: usize) {
    let width = (n as f64).sqrt() as u16;
    let height = width;
    let mut grid = Grid::new(width, height);
    for y in 0..height {
        for x in 0..width {
            grid.cell_mut(x, y).set_symbol("A");
        }
    }
    divan::black_box(&grid);
}

#[divan::bench]
fn full_grid_render() -> Grid {
    let mut grid = Grid::new(80, 24);
    render_demo_scene(&mut grid);
    grid
}

#[divan::bench]
fn diff_80x24() {
    let prev = Grid::filled(80, 24, Cell::new("a"));
    let next = Grid::filled(80, 24, Cell::new("b"));
    let diff: Vec<_> = divan::black_box(prev.diff(&next)).collect();
    divan::black_box(diff);
}
```

### Running Benchmarks

```bash
# criterion
cargo bench --bench grid_benchmarks
# Results in target/criterion/ with HTML reports

# divan
cargo bench --bench grid_benchmarks
# Table output in terminal

# Compare against baseline (criterion)
cargo bench --bench grid_benchmarks -- --save-baseline main
# After changes:
cargo bench --bench grid_benchmarks -- --baseline main
```

### Recommendation

Use **divan** for its simpler API and built-in allocation tracking. Use **criterion** if you need
HTML reports, statistical regression detection with configurable confidence levels, or integration
with continuous benchmarking services (e.g., codspeed, bencher.dev).

[Source: criterion docs](https://docs.rs/criterion/), [divan docs](https://docs.rs/divan/)

---

## 8. How Ratatui Tests Widgets

### The `assert_buffer_lines` Pattern

Ratatui tests widgets by rendering them into a `Buffer` (not through the full `Terminal` / `Backend`
pipeline) and comparing the buffer content line by line:

```rust
#[test]
fn test_list_widget() {
    let items = vec!["Item 1", "Item 2", "Item 3"];
    let list = List::new(items);

    let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
    list.render(buf.area, &mut buf);

    let expected = Buffer::with_lines([
        "Item 1    ",
        "Item 2    ",
        "Item 3    ",
    ]);
    assert_eq!(buf, expected);
}
```

### Buffer::with_lines

The core helper is `Buffer::with_lines`, which constructs an expected buffer from string slices. It
handles:

- Width inference (maximum line width)
- Height inference (number of lines)
- Unicode width calculation (wide characters take 2 cells)
- Styled text via `Line` conversion

```rust
// Styled expected output
let expected = Buffer::with_lines([
    "Header".bold(),
    "Normal text".into(),
    "Colored".red(),
]);
```

### The Deprecated assert_buffer_eq! Macro

Ratatui previously used `assert_buffer_eq!` which provided detailed diff output on failure:

```rust
// Now deprecated in favor of standard assert_eq!
assert_buffer_eq!(&actual_buffer, &expected_buffer);
```

The macro compared areas first, then cell-by-cell content, producing output like:

```
buffer contents not equal
diff:
0: at (2, 1)
  expected: Cell { symbol: "I", ... }
  actual:   Cell { symbol: "i", ... }
```

The current recommendation is `assert_eq!(&actual, &expected)` since `Buffer` implements `Debug`
with a readable format showing content and style changes.

### TestBackend-Level Testing

For integration tests that exercise the full render pipeline:

```rust
#[test]
fn test_full_render_pipeline() {
    let backend = TestBackend::new(20, 5);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| {
        let list = List::new(vec!["A", "B", "C"]);
        frame.render_widget(list, frame.area());
    }).unwrap();

    terminal.backend().assert_buffer_lines([
        "A                   ",
        "B                   ",
        "C                   ",
        "                    ",
        "                    ",
    ]);
}
```

### rstest for Parameterized Tests

Ratatui uses `rstest` extensively for parameterized/table-driven tests:

```rust
use rstest::rstest;

#[rstest]
#[case([A, B, C, D, E], 0..5, 2, [A, B],    [C, D, E, S, S])]
#[case([A, B, C, D, E], 0..5, 5, [A,B,C,D,E], [S, S, S, S, S])]
#[case([A, B, C, D, E], 1..4, 2, [],         [A, D, S, S, E])]
fn scroll_region_up(
    #[case] initial_screen: [&'static str; 5],
    #[case] range: Range<u16>,
    #[case] scroll_by: u16,
    #[case] expected_scrollback: impl IntoIterator<Item = &'static str>,
    #[case] expected_buffer: impl IntoIterator<Item = &'static str>,
) {
    let mut backend = TestBackend::with_lines(initial_screen);
    backend.scroll_region_up(range, scroll_by).unwrap();
    backend.assert_scrollback_lines(expected_scrollback);
    backend.assert_buffer_lines(expected_buffer);
}
```

[Source: ratatui buffer.rs, test.rs, assert.rs](https://github.com/ratatui/ratatui/tree/main/ratatui-core/src)

---

## 9. How Crossterm Tests Terminal Operations

### Overview

Crossterm is a pure-Rust cross-platform terminal manipulation library. Its testing approach differs
from ratatui because it operates at the I/O level rather than the buffer level.

### Testing Strategy

1. **Unit tests for data structures**: `KeyEvent`, `MouseEvent`, `Color` parsing, command
   serialization are tested with standard unit tests

2. **Platform-specific conditional compilation**: tests that interact with the terminal are gated
   behind `#[cfg(unix)]` or `#[cfg(windows)]`

3. **Command serialization tests**: verify that `Command` implementations produce correct ANSI
   escape sequences:

   ```rust
   #[test]
   fn test_move_to_command() {
       let mut buf = Vec::new();
       MoveTo(5, 10).write_ansi(&mut buf).unwrap();
       assert_eq!(buf, b"\x1b[11;6H");  // ANSI is 1-indexed
   }
   ```

4. **Event parsing tests**: verify that byte sequences are correctly parsed into events:

   ```rust
   #[test]
   fn test_parse_csi_cursor_position() {
       let input = b"\x1b[5;10R";
       let event = parse_event(input).unwrap();
       assert_eq!(event, InternalEvent::CursorPosition(9, 4));
   }
   ```

5. **Integration tests with real terminals**: some tests require `--ignored` flag and a real
   terminal:

   ```rust
   #[test]
   #[ignore]  // requires real terminal
   fn test_raw_mode() {
       enable_raw_mode().unwrap();
       assert!(is_raw_mode_enabled());
       disable_raw_mode().unwrap();
   }
   ```

### Cross-Platform CI Matrix

Crossterm tests on multiple platforms since terminal behavior varies:

- Ubuntu (Linux)
- macOS (Intel and Apple Silicon)
- Windows (Console Host and Windows Terminal)

Platform-specific exclusions (e.g., termion tests excluded on Windows) are handled in the CI matrix.

[Source: crossterm repository](https://github.com/crossterm-rs/crossterm)

---

## 10. CI Configuration for Running All Test Types

### Reference: Ratatui CI Structure

Ratatui's CI is one of the most comprehensive in the Rust TUI ecosystem. Key jobs:

| Job               | Purpose                                                                                | Blocking                    |
| ----------------- | -------------------------------------------------------------------------------------- | --------------------------- |
| `lint-formatting` | `cargo fmt --check` (nightly)                                                          | Yes                         |
| `lint-typos`      | typo detection with `crate-ci/typos`                                                   | Yes                         |
| `cargo-deny`      | license/advisory/ban checks                                                            | Advisory: continue-on-error |
| `cargo-machete`   | unused dependency detection                                                            | Yes                         |
| `lint-clippy`     | clippy on stable + beta (beta non-blocking)                                            | Stable: yes                 |
| `lint-markdown`   | markdownlint on all .md files                                                          | Yes                         |
| `coverage`        | `cargo-llvm-cov` with codecov upload                                                   | Yes                         |
| `check`           | `cargo check` on {ubuntu, windows, macos} x {MSRV, stable}                             | Yes                         |
| `build-no-std`    | build for `x86_64-unknown-none` target                                                 | Yes                         |
| `test-docs`       | `cargo test --doc`                                                                     | Yes                         |
| `test-libs`       | library unit tests on {MSRV, stable}                                                   | Yes                         |
| `test-backends`   | per-backend tests on {ubuntu, windows, macos} x {crossterm, termion, termina, termwiz} | Yes                         |
| `required`        | aggregation gate checking all jobs passed                                              | Merge gate                  |

### Recommended CI Configuration for a New Library

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

concurrency:
  group: ${{ github.workflow }}-${{ github.head_ref || github.run_id }}
  cancel-in-progress: true

jobs:
  # Fast feedback: formatting and linting
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with: { components: rustfmt, clippy }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets --all-features -- -D warnings

  # Unit and integration tests
  test:
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        rust: [stable, "1.80.0"]  # MSRV
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: ${{ matrix.rust }} }
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all-features

  # Snapshot tests (insta)
  snapshots:
    runs-on: ubuntu-latest
    env:
      INSTA_UPDATE: no  # fail on mismatch, don't write .snap.new
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all-features

  # Property-based tests (extended run in CI)
  proptest:
    runs-on: ubuntu-latest
    env:
      PROPTEST_CASES: 10000  # more cases than default 256
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all-features -- --ignored proptest

  # Fuzz smoke test (main branch only)
  fuzz:
    if: github.ref == 'refs/heads/main'
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target: [parse_ansi, parse_config]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: actions/cache@v4
        with:
          path: ${{ runner.tool_cache }}/cargo-fuzz
          key: cargo-fuzz-0.12.0
      - run: |
          echo "${{ runner.tool_cache }}/cargo-fuzz/bin" >> $GITHUB_PATH
          cargo install --root "${{ runner.tool_cache }}/cargo-fuzz" --version 0.12.0 cargo-fuzz --locked
      - run: cargo fuzz run ${{ matrix.target }} -- -max_total_time=300
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: fuzz-${{ matrix.target }}
          path: fuzz/artifacts

  # Benchmarks (no regression gate, just track)
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo bench --all-features -- --output-format=bencher | tee bench-results.txt
      - uses: actions/upload-artifact@v4
        with:
          name: bench-results
          path: bench-results.txt

  # Coverage
  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: llvm-tools }
      - uses: taiki-e/install-action@v2
        with: { tool: cargo-llvm-cov }
      - uses: Swatinem/rust-cache@v2
      - run: cargo llvm-cov --all-features --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v5
        with:
          files: lcov.info
          token: ${{ secrets.CODECOV_TOKEN }}

  # Merge gate
  required:
    runs-on: ubuntu-latest
    needs: [lint, test, snapshots, coverage]
    if: always()
    steps:
      - run: |
          echo '${{ toJson(needs) }}'
          test '${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') }}' = 'false'
```

### Key Patterns

1. **Concurrency control**: cancel in-progress runs for the same PR
2. **Fast-fail ordering**: lint jobs run first, catch trivial issues before full matrix
3. **MSRV testing**: test against minimum supported Rust version
4. **Cross-platform matrix**: {os} x {toolchain} but with sensible exclusions
5. **Fuzz on main only**: expensive fuzz runs only on merge, not every PR
6. **Benchmarks as artifacts**: track but don't gate on benchmark results
7. **Merge gate job**: single `required` job that checks all others passed

[Source: ratatui CI](https://github.com/ratatui/ratatui/blob/main/.github/workflows/ci.yml),
[Rust Fuzz Book CI](https://rust-fuzz.github.io/book/cargo-fuzz/ci.html)

---

## Sources

### Kept

- **ratatui TestBackend**
  (<https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend/test.rs>) - primary
  reference for test backend design, assert_buffer_lines pattern, scrolling region tests
- **ratatui buffer.rs**
  (<https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/buffer/buffer.rs>) - Buffer
  struct, diff algorithm, with_lines constructor, comprehensive test suite with rstest
- **ratatui assert.rs**
  (<https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/buffer/assert.rs>) -
  assert_buffer_eq! macro (now deprecated)
- **ratatui CI** (<https://github.com/ratatui/ratatui/blob/main/.github/workflows/ci.yml>) -
  multi-backend, multi-OS CI configuration
- **insta docs** (<https://docs.rs/insta/>) - snapshot testing API, workflow, configuration
- **Proptest Book** (<https://proptest-rs.github.io/proptest/>) - property-based testing strategies
  and shrinking
- **Rust Fuzz Book** (<https://rust-fuzz.github.io/book/>) - cargo-fuzz setup, structure-aware
  fuzzing, CI integration
- **criterion docs** (<https://docs.rs/criterion/>) - statistical benchmarking framework
- **divan docs** (<https://docs.rs/divan/>) - attribute-macro benchmarking with allocation profiling
- **crossterm** (<https://github.com/crossterm-rs/crossterm>) - cross-platform terminal library
  testing context

### Dropped

- Generic Rust testing tutorials - too basic, no terminal-specific content
- GitHub page chrome/navigation content - not actual source content (GitHub requires auth for
  rendered views)

---

## Gaps

1. **Crossterm test internals**: could not access crossterm's actual test files (event parsing
   tests, command serialization tests) due to GitHub rendering issues. The description above is
   inferred from the codebase structure and API patterns.

2. **Visual regression tooling**: no established Rust crate specifically for terminal-to-PNG visual
   regression testing exists. This would need custom implementation using `image` + a font
   rasterizer (e.g., `ab_glyph`, `cosmic-text`).

3. **Continuous benchmarking services**: did not research integration with services like codspeed or
   bencher.dev that track benchmark results over time and can flag regressions in PRs.

4. **Miri for unsafe code**: if the grid library uses `unsafe` (e.g., for performance-critical cell
   access), running tests under Miri (`cargo +nightly miri test`) should be part of CI.

5. **Mutation testing**: tools like `cargo-mutants` could verify test suite quality by introducing
   mutations and checking they're caught. Not covered here.
