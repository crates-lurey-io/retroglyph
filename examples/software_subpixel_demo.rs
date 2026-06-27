//! DVD-style bouncing `@` screensaver with sub-pixel offsets and layers.
//!
//! Demonstrates the software backend's multi-layer compositing and sub-cell
//! pixel offset features: a large `@` ping-pongs diagonally across the grid,
//! bouncing off the walls like a classic TV screensaver.
//!
//! Run with:
//!   `cargo run --example software_subpixel_demo --features software-default-font`

mod util;

use retroglyph::Terminal;
use retroglyph::backend::software::SoftwareBackendBuilder;
use retroglyph::color::Color;
use retroglyph::event::{Event, KeyCode};
use std::time::Duration;
use util::lcg::Lcg;

// ── State ────────────────────────────────────────────────────────────────────

struct BounceState {
    x: u16,
    y: u16,
    dx: i16,
    dy: i16,
    color: Color,
    frame: u64,
}

impl BounceState {
    fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut rng = Lcg::from_time();
        #[cfg(target_arch = "wasm32")]
        let mut rng = Lcg::new(42);

        #[allow(clippy::cast_possible_truncation)]
        let start_x = (rng.next() % 26 + 7) as u16;
        #[allow(clippy::cast_possible_truncation)]
        let start_y = (rng.next() % 9 + 3) as u16;
        let dir_x: i16 = if rng.next().is_multiple_of(2) { 1 } else { -1 };
        let dir_y: i16 = if rng.next().is_multiple_of(2) { 1 } else { -1 };
        let color = pick_color(&mut rng);

        Self {
            x: start_x,
            y: start_y,
            dx: dir_x,
            dy: dir_y,
            color,
            frame: 0,
        }
    }
}

// ── Color helpers ─────────────────────────────────────────────────────────────

fn pick_color(rng: &mut Lcg) -> Color {
    const COLORS: &[Color] = &[
        Color::BRIGHT_RED,
        Color::BRIGHT_GREEN,
        Color::BRIGHT_YELLOW,
        Color::BRIGHT_BLUE,
        Color::BRIGHT_MAGENTA,
        Color::BRIGHT_CYAN,
        Color::BRIGHT_WHITE,
        Color::Rgb {
            r: 255,
            g: 128,
            b: 0,
        },
        Color::Rgb {
            r: 128,
            g: 0,
            b: 255,
        },
        Color::Rgb {
            r: 0,
            g: 255,
            b: 128,
        },
    ];
    #[allow(clippy::cast_possible_truncation)]
    COLORS[(rng.next() as usize) % COLORS.len()]
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn draw_background(term: &mut Terminal<impl retroglyph::Backend>) {
    let size = term.size();
    term.layer(0);
    for y in 0..size.height {
        for x in 0..size.width {
            let (fg, ch) = if (x + y) % 2 == 0 {
                (
                    Color::Rgb {
                        r: 70,
                        g: 60,
                        b: 90,
                    },
                    '#',
                )
            } else {
                (
                    Color::Rgb {
                        r: 100,
                        g: 80,
                        b: 50,
                    },
                    '.',
                )
            };
            term.fg(fg);
            term.bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 15,
            });
            term.put(x, y, ch);
        }
    }
}

/// Run one frame. Returns `false` when the user quits.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
fn tick(term: &mut Terminal<impl retroglyph::Backend>, s: &mut BounceState) -> bool {
    let size = term.size();

    draw_background(term);

    term.layer(1);

    let right = i64::from(size.width) - 1;
    let bottom = i64::from(size.height) - 1;

    let pos_x = i64::from(s.x) + i64::from(s.dx);
    let pos_y = i64::from(s.y) + i64::from(s.dy);

    if pos_x <= 0 || pos_x >= right {
        s.dx = -s.dx;
        s.color = pick_color(&mut Lcg::new(s.frame));
    }
    if pos_y <= 0 || pos_y >= bottom {
        s.dy = -s.dy;
        s.color = pick_color(&mut Lcg::new(s.frame.wrapping_add(1)));
    }

    s.x = (i64::from(s.x) + i64::from(s.dx)).clamp(1, right - 1) as u16;
    s.y = (i64::from(s.y) + i64::from(s.dy)).clamp(1, bottom - 1) as u16;

    // Sub-pixel offset: sawtooth -6 .. +6.
    let sub_cycle = (s.frame as i64) % 14;
    let off_x = ((sub_cycle - 6).abs()) as i16;
    let off_y = {
        let c = (s.frame.wrapping_add(7) as i64) % 14;
        ((c - 6).abs()) as i16
    };

    term.fg(s.color);
    term.put_offset(s.x, s.y, off_x, off_y, '@');

    // Layer 2: header bar
    term.layer(2);
    let header_bg = Color::Rgb {
        r: 30,
        g: 30,
        b: 45,
    };
    for x in 0..size.width {
        term.put_styled(x, 0, ' ', retroglyph::Style::new().bg(header_bg));
    }
    let header = "rg DVD screensaver [Esc to quit]";
    let header_style = retroglyph::Style::new()
        .fg(Color::BRIGHT_WHITE)
        .bg(header_bg);
    #[allow(clippy::cast_possible_truncation)]
    let hx = size.width.saturating_sub(header.len() as u16) / 2;
    for (i, ch) in header.chars().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        term.put_styled(hx + i as u16, 0, ch, header_style);
    }

    // Layer 2: footer status
    let footer_bg = Color::Rgb {
        r: 30,
        g: 30,
        b: 45,
    };
    for x in 0..size.width {
        term.put_styled(
            x,
            size.height - 1,
            ' ',
            retroglyph::Style::new().bg(footer_bg),
        );
    }
    let footer = format!(
        "pos ({},{})  off ({off_x},{off_y})  frame {}",
        s.x, s.y, s.frame
    );
    #[allow(clippy::cast_possible_truncation)]
    let fx = size.width.saturating_sub(footer.len() as u16) / 2;
    term.fg(Color::BRIGHT_WHITE);
    term.bg(footer_bg);
    term.print(fx, size.height - 1, &footer);

    term.present().expect("present failed");
    s.frame = s.frame.wrapping_add(1);

    if let Some(event) = term.poll(Duration::from_millis(16)) {
        match event {
            Event::Key(k) if k.code == KeyCode::Escape => return false,
            Event::Close => return false,
            _ => {}
        }
    }

    true
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_main() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    main();
    Ok(())
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let backend = SoftwareBackendBuilder::new()
        .title(env!("CARGO_BIN_NAME"))
        .grid_size(40, 15)
        .scale(4)
        .build()
        .expect("backend init failed (try the `software-default-font` feature)");

    let mut state: Option<BounceState> = None;
    let mut quit = false;
    backend
        .run_windowed(move |term: &mut Terminal<_>| {
            if quit {
                return;
            }
            let s = state.get_or_insert_with(BounceState::new);
            if !tick(term, s) {
                quit = true;
                #[cfg(not(target_arch = "wasm32"))]
                std::process::exit(0);
            }
        })
        .expect("event loop failed");
}
