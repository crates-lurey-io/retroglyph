//! E2E snapshot tests for the crossterm backend.
//!
//! Spawns `crossterm_demo` in a real PTY, feeds it input, parses the
//! resulting ANSI output with a proper VT100 emulator, and renders the
//! final screen state to SVG for visual regression testing.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const ROWS: u16 = 15;
const COLS: u16 = 60;
const CHAR_PX: f64 = 8.4;
const LINE_PX: f64 = 18.0;
const PAD: f64 = 8.0;

fn example_bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop(); // deps/
    path.pop(); // debug/
    path.push("examples");
    path.push(name);
    path
}

/// Spawn `bin` in a PTY, send `setup` input to navigate to the desired state,
/// wait for the screen to settle, capture output, then send `quit` to terminate.
fn capture_pty(bin: &Path, setup: &[u8], quit: &[u8], rows: u16, cols: u16) -> Vec<u8> {
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

    // Let the process start and draw its first frame.
    std::thread::sleep(std::time::Duration::from_millis(200));
    writer.write_all(setup).expect("write setup");
    // Let it process the navigation input and redraw.
    std::thread::sleep(std::time::Duration::from_millis(150));
    writer.write_all(quit).expect("write quit");
    drop(writer);

    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).expect("read to end");
    let _ = child.wait();
    buf
}

// --- SVG rendering ----------------------------------------------------------

fn css_color(color: vt100::Color, default: &str) -> String {
    match color {
        vt100::Color::Default => default.to_owned(),
        vt100::Color::Idx(idx) => ansi_palette(idx).to_owned(),
        vt100::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct Run {
    col: u16,
    text: String,
    fg: String,
    bg: String,
    bold: bool,
    italic: bool,
}

/// Collect cells in a row into consecutive style runs.
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

fn render_svg(screen: &vt100::Screen, rows: u16, cols: u16) -> String {
    let w = PAD.mul_add(2.0, f64::from(cols) * CHAR_PX);
    let h = PAD.mul_add(2.0, f64::from(rows) * LINE_PX);

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#111111\"/>\n"
    );

    for row in 0..rows {
        let y = (f64::from(row) + 1.0).mul_add(LINE_PX, PAD);
        writeln!(
            out,
            "<text font-family=\"monospace\" font-size=\"14\" y=\"{y:.1}\">"
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

// --- Tests ------------------------------------------------------------------

#[test]
fn test_crossterm_demo_snapshot() {
    let bin = example_bin("crossterm_demo");
    assert!(bin.exists(), "crossterm_demo binary not found at {bin:?}");

    // Move right twice, down twice, then quit.
    let raw = capture_pty(&bin, b"ddss", b"q", ROWS, COLS);

    let mut parser = vt100::Parser::new(ROWS, COLS, 0);

    // Feed bytes incrementally. The demo uses the alternate screen buffer, so
    // `parser.screen()` becomes blank once the process exits and issues
    // LeaveAlternateScreen. We therefore track the last offset at which the
    // alternate screen was still active and replay up to that point for the
    // final snapshot.
    let mut last_alternate_end = 0usize;
    let mut pos = 0usize;
    while pos < raw.len() {
        let end = (pos + 64).min(raw.len());
        parser.process(&raw[pos..end]);
        if parser.screen().alternate_screen() {
            last_alternate_end = end;
        }
        pos = end;
    }

    let mut snap_parser = vt100::Parser::new(ROWS, COLS, 0);
    snap_parser.process(&raw[..last_alternate_end]);

    let svg = render_svg(snap_parser.screen(), ROWS, COLS);

    assert!(svg.contains("HP:"), "status bar missing from SVG output");

    // Write a standalone SVG next to the snap so GitHub renders it in PR diffs.
    let svg_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/crossterm_demo.svg");
    std::fs::write(&svg_path, &svg).expect("write SVG");

    insta::assert_snapshot!(svg);
}
