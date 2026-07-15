//! Shared snapshot-test harness for retroglyph's examples.
//!
//! Every example gets three snapshot artifacts, all driven from its own
//! `Example` impl (no separate "test version" of the example to drift out of
//! sync):
//!
//! - [`headless_snapshot`]: plain text, via [`Headless::format_view`] --
//!   cheap, no subprocess, runs on every target including `wasm32`.
//! - [`png_snapshot`]: a pixel-level render via `SoftwareRenderer`'s headless
//!   mode, encoded as PNG bytes -- pass straight to
//!   `insta::assert_binary_snapshot!(".png", ...)`, which compares the
//!   bytes byte-for-byte against the committed `.snap.png` (this is exact,
//!   not a perceptual/fuzzy image diff, so it can only ever false-positive
//!   on a genuinely non-deterministic render, not on incidental PNG
//!   re-encoding -- we always encode with the same `image` crate call).
//! - [`svg_snapshot`]: spawns the compiled `--features crossterm` example
//!   binary in a real PTY, drives it with real ANSI input, and renders the
//!   resulting `vt100` screen to SVG -- the only one of the three that
//!   exercises the actual terminal I/O path end to end. Both `insta::assert_
//!   snapshot!`-diffed (via [`svg_snapshot`]'s returned string) *and* written
//!   to a plain `.svg` file via [`write_snapshot_file`] for visual review
//!   (opening the raw `.snap` text file directly wouldn't render, since
//!   insta prepends a YAML header that isn't valid SVG).
//!
//! [`Headless::format_view`]: retroglyph_core::Headless::format_view

#![allow(dead_code)] // not every test file uses every helper

use retroglyph_examples::Example;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Runs `E`'s headless fallback for up to `frames` frames and returns each
/// frame's rendered text, joined by a separator line -- pass straight to
/// `insta::assert_snapshot!`.
#[must_use]
pub fn headless_snapshot<E: Example>(frames: u32) -> String {
    retroglyph_examples::render_headless_frames::<E>(frames).join("\n--- frame ---\n")
}

/// Builds a `SoftwareRenderer` (`cols`x`rows` grid, embedded default font, `scale`, plus
/// whatever `E::configure_software` adds -- a tileset, most likely), runs `E::init` + one
/// `E::tick`, and PNG-encodes the resulting pixel buffer.
///
/// Threading every example's own [`Example::configure_software`] through here (rather than
/// building a bare, uncustomized renderer) keeps this snapshot honest: it's built from exactly
/// the same builder `cargo run --example <name> --features software` would use, not a simplified
/// stand-in that could drift from it (e.g. missing a registered tileset, so the PNG never
/// actually exercises sprite rendering at all).
///
/// Requires the `software` feature on `retroglyph-examples` (which pulls in
/// `retroglyph-software/default-font` and `retroglyph-software/tilesets` -- see the Cargo.toml
/// comment).
///
/// # Panics
///
/// Panics if the software backend or PNG encoding fails.
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[must_use]
pub fn png_snapshot<E: Example>(cols: u16, rows: u16, scale: u8) -> Vec<u8> {
    use retroglyph_core::Terminal;
    use retroglyph_software::SoftwareBackendBuilder;
    use retroglyph_window::Presenter;

    let builder = E::configure_software(
        SoftwareBackendBuilder::new()
            .grid_size(cols, rows)
            .scale(scale),
    );
    let renderer = builder
        .build()
        .expect("software backend init")
        .run_headless();

    // Read the pixel-buffer geometry before handing `renderer` to `Terminal`
    // (which owns it from here): cols/rows in cells x the presenter's own
    // reported cell size in pixels.
    let (cell_w, cell_h) = renderer.cell_size();
    let width = u32::from(cols) * cell_w;
    let height = u32::from(rows) * cell_h;

    let mut term = Terminal::new(renderer);
    let mut state = E::init(&mut term);
    let frame = retroglyph_core::Frame {
        delta: retroglyph_examples::HEADLESS_FRAME_DELTA,
        frame: 0,
    };
    state.tick(&mut term, &frame);

    let mut rgb = Vec::with_capacity(term.backend().pixels().len() * 3);
    for &p in term.backend().pixels() {
        rgb.push(((p >> 16) & 0xff) as u8);
        rgb.push(((p >> 8) & 0xff) as u8);
        rgb.push((p & 0xff) as u8);
    }

    let img: image::RgbImage =
        image::ImageBuffer::from_raw(width, height, rgb).expect("pixel buffer matches dimensions");
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("PNG encode");
    out
}

/// A build directory dedicated to [`build_crossterm_example`]'s output, isolated from the
/// workspace's normal `target/` dir.
///
/// `cargo test --workspace --all-features` (see this crate's `AGENTS.md`/`docs/testing.md`)
/// compiles every `[[example]]` -- including with the `software` feature, which
/// [`build_crossterm_example`]'s own doc comment already explains is unusable in a PTY -- into
/// the ordinary `target/debug/examples/` directory *before* any `#[test]` runs. Building the
/// crossterm-only variant back into that same path (as this used to do) means every single
/// `svg_snapshot` test forces a real relink there, fighting the `--all-features` build for the
/// same output file on every `cargo test` invocation, warm cache or not.
///
/// That relink churn isn't just wasted compile time: on macOS, first executing a binary the
/// kernel hasn't seen this exact content at (a fresh relink counts) triggers a synchronous
/// code-signature validation that measured ~1.5-2 real seconds per example here -- almost all of
/// `svg_snapshot`'s wall time for the smaller examples. Routing this crate's crossterm builds to
/// their own `--target-dir` sidesteps both costs at once: they're never touched by the
/// `--all-features` build, so once each example has been built here (and executed once, paying
/// that validation cost a single time ever), every later `cargo test` run -- this one included --
/// finds a byte-identical, already-validated binary and skips straight to the real capture.
#[cfg(not(target_arch = "wasm32"))]
fn crossterm_target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("pty-examples")
}

/// Builds *every* `[[example]]` with `--features crossterm` into [`crossterm_target_dir`]
/// (regardless of what features this test binary itself was compiled with -- `cargo test
/// --all-features` would otherwise leave a GUI (`software`) binary on disk that hangs when spawned
/// in a PTY) and returns `example_name`'s path.
///
/// Builds `--examples` (plural, i.e. every example target) rather than just `example_name`
/// deliberately: this is called once per `svg_snapshot` test, and those tests are their own
/// separate `[[test]]` binaries (15 of them, one per example) that `cargo test`/`cargo nextest`
/// runs as 15 separate processes -- serially under plain `cargo test`, concurrently under
/// nextest. Building only the caller's own example would mean 15 separate `cargo build`
/// invocations, each re-walking the whole dependency graph and each paying its own fingerprint
/// check. Building everything on every call instead means the *first* call (whichever process
/// gets there first -- Cargo's own target-dir lock makes this safe under nextest's concurrent
/// processes too) does one real build of all 15 examples, and every subsequent call across every
/// other test binary just confirms freshness and returns immediately.
///
/// # Panics
///
/// Panics if the build fails.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn build_crossterm_example(example_name: &str) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = env!("CARGO_MANIFEST_DIR");
    let target_dir = crossterm_target_dir();
    let status = std::process::Command::new(&cargo)
        .args([
            "build",
            "--manifest-path",
            &format!("{manifest}/Cargo.toml"),
            "--examples",
            "--features",
            "crossterm",
            "--target-dir",
        ])
        .arg(&target_dir)
        .status()
        .expect("failed to run cargo build");
    assert!(
        status.success(),
        "cargo build --examples --features crossterm failed"
    );
    target_dir.join("debug").join("examples").join(example_name)
}

/// Spawns `bin` in a PTY sized `rows`x`cols`, writes `input`, waits until the
/// screen contains `ready_marker`, then closes the writer (EOF) and returns
/// all captured output bytes.
///
/// # Panics
///
/// Panics if the PTY can't be opened, or if `ready_marker` never appears
/// within 10 seconds.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn capture_pty(bin: &Path, input: &[u8], rows: u16, cols: u16, ready_marker: &str) -> Vec<u8> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut cmd = CommandBuilder::new(bin);
    cmd.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("reader");
    let mut writer = pair.master.take_writer().expect("writer");

    let output = Arc::new(Mutex::new(Vec::new()));
    let output_clone = Arc::clone(&output);
    let reader_done = Arc::new(Mutex::new(false));
    let reader_done_clone = Arc::clone(&reader_done);
    // Keep the `JoinHandle` (rather than only the `reader_done` flag below): setting that flag
    // and this closure actually *returning* (dropping `output_clone`, its `Arc` clone) are two
    // separate steps, so polling the flag alone races the still-unwinding thread -- the poll can
    // observe `true` and reach `Arc::try_unwrap` below a moment before `output_clone` is actually
    // dropped, which fails non-deterministically (this is what used to intermittently panic
    // here). `.join()` blocks until the thread function has fully returned, which is the only way
    // to be sure the second `Arc` reference is gone.
    let reader_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => output_clone.lock().unwrap().extend_from_slice(&buf[..n]),
            }
        }
        *reader_done_clone.lock().unwrap() = true;
        drop(reader);
    });

    writer.write_all(input).expect("write input");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if String::from_utf8_lossy(&output.lock().unwrap()).contains(ready_marker) {
            break;
        }
        assert!(
            Instant::now() <= deadline,
            "timed out waiting for {ready_marker:?} in PTY output"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    writer.write_all(b"q").expect("write quit");
    drop(writer);
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if *reader_done.lock().unwrap() {
            break;
        }
        assert!(Instant::now() <= deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    reader_handle.join().expect("reader thread panicked");

    Arc::try_unwrap(output).unwrap().into_inner().unwrap()
}

/// Feeds `raw` PTY bytes through a fresh `vt100::Parser` and renders the
/// final alternate-screen contents as SVG.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn svg_snapshot(raw: &[u8], rows: u16, cols: u16) -> String {
    let mut last_alternate_end = 0usize;
    let mut pos = 0usize;
    let mut parser = vt100::Parser::new(rows, cols, 0);
    while pos < raw.len() {
        let end = (pos + 64).min(raw.len());
        parser.process(&raw[pos..end]);
        if parser.screen().alternate_screen() {
            last_alternate_end = end;
        }
        pos = end;
    }

    let mut snap_parser = vt100::Parser::new(rows, cols, 0);
    snap_parser.process(&raw[..last_alternate_end]);
    render_svg(snap_parser.screen(), rows, cols)
}

#[cfg(not(target_arch = "wasm32"))]
const CHAR_PX: f64 = 8.4;
#[cfg(not(target_arch = "wasm32"))]
const LINE_PX: f64 = 18.0;
#[cfg(not(target_arch = "wasm32"))]
const PAD: f64 = 8.0;

#[cfg(not(target_arch = "wasm32"))]
fn css_color(color: vt100::Color, default: &str) -> String {
    match color {
        vt100::Color::Default => default.to_owned(),
        vt100::Color::Idx(idx) => ansi_palette(idx).to_owned(),
        vt100::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
const fn ansi_palette(idx: u8) -> &'static str {
    match idx {
        0 => "#000000",
        1 => "#800000",
        2 => "#008000",
        3 => "#808000",
        4 => "#000080",
        5 => "#800080",
        6 => "#008080",
        7 => "#c0c0c0",
        8 => "#808080",
        9 => "#ff5555",
        10 => "#55ff55",
        11 => "#ffff55",
        12 => "#5555ff",
        13 => "#ff55ff",
        14 => "#55ffff",
        15 => "#ffffff",
        _ => "#888888",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(not(target_arch = "wasm32"))]
struct Run {
    col: u16,
    text: String,
    fg: String,
    bg: String,
    bold: bool,
    italic: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_runs(screen: &vt100::Screen, row: u16, cols: u16) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        let fg = css_color(cell.fgcolor(), "#cccccc");
        let bg = css_color(cell.bgcolor(), "none");
        let bold = cell.bold();
        let italic = cell.italic();
        let contents = cell.contents();
        let glyph = if contents.is_empty() {
            " ".to_owned()
        } else {
            contents
        };

        let extends_current = runs.last().is_some_and(|prev: &Run| {
            prev.fg == fg && prev.bg == bg && prev.bold == bold && prev.italic == italic
        });

        if extends_current {
            if let Some(last) = runs.last_mut() {
                last.text.push_str(&glyph);
            }
        } else {
            runs.push(Run {
                col,
                text: glyph,
                fg,
                bg,
                bold,
                italic,
            });
        }
    }

    runs
}

#[cfg(not(target_arch = "wasm32"))]
fn tspan_attrs(run: &Run) -> String {
    let x = f64::from(run.col).mul_add(CHAR_PX, PAD);
    let mut attrs = format!("fill=\"{}\" x=\"{x:.1}\"", run.fg);
    if run.bold {
        attrs.push_str(" font-weight=\"bold\"");
    }
    if run.italic {
        attrs.push_str(" font-style=\"italic\"");
    }
    if run.bg != "none" {
        write!(attrs, " style=\"background:{}\"", run.bg).expect("String write");
    }
    attrs
}

#[cfg(not(target_arch = "wasm32"))]
fn render_svg(screen: &vt100::Screen, rows: u16, cols: u16) -> String {
    let w = PAD.mul_add(2.0, f64::from(cols) * CHAR_PX);
    let h = PAD.mul_add(2.0, f64::from(rows) * LINE_PX);

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#111111\"/>\n"
    );

    for row in 0..rows {
        let y = (f64::from(row) + 1.0).mul_add(LINE_PX, PAD);
        // xml:space="preserve": without it, SVG collapses runs of
        // whitespace, so a run of leading spaces used to indent text to its
        // real column (see `collect_runs`/`tspan_attrs`, which position runs
        // by literal spaces rather than per-character x offsets) silently
        // collapses to nothing and the text renders left-aligned instead of
        // at its actual terminal column.
        writeln!(
            out,
            "<text font-family=\"monospace\" font-size=\"14\" y=\"{y:.1}\" xml:space=\"preserve\">"
        )
        .expect("String write");
        for run in collect_runs(screen, row, cols) {
            writeln!(
                out,
                "  <tspan {}>{}</tspan>",
                tspan_attrs(&run),
                xml_escape(&run.text)
            )
            .expect("String write");
        }
        out.push_str("</text>\n");
    }

    out.push_str("</svg>\n");
    out
}

/// Writes `bytes` to `tests/snapshots/<file_name>`, creating the directory
/// if needed. Used for the PNG/SVG artifacts: reviewed visually (`git diff`
/// on a binary/SVG file, or opening the PNG), not text-diffed by insta.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_snapshot_file(file_name: &str, bytes: &[u8]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    std::fs::create_dir_all(&dir).expect("create snapshots dir");
    std::fs::write(dir.join(file_name), bytes).expect("write snapshot file");
}
