//! DVD-style bouncing `@` screensaver with sub-pixel offsets and layers.
//!
//! Demonstrates the software backend's multi-layer compositing and sub-cell
//! pixel offset features: a large `@` ping-pongs diagonally across the grid,
//! bouncing off the walls like a classic TV screensaver.
//!
//! Run with:
//!   `cargo run --example software_subpixel_demo --features software-default-font`

use rg::Terminal;
use rg::backend::software::SoftwareBackendBuilder;
use rg::color::Color;
use rg::event::{Event, KeyCode};
use std::time::Duration;

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn main() {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42);

    let backend = SoftwareBackendBuilder::new()
        .title("rg sub-pixel DVD screensaver")
        .grid_size(40, 15)
        .scale(4)
        .build()
        .expect("backend init failed (try the `software-default-font` feature)");

    #[allow(clippy::cast_possible_truncation)]
    let mut rng = Lcg::new(seed);

    #[allow(clippy::cast_possible_truncation)]
    let start_x = (rng.next() % 26 + 7) as u16;
    #[allow(clippy::cast_possible_truncation)]
    let start_y = (rng.next() % 9 + 3) as u16;

    let dir_x: i16 = if rng.next() % 2 == 0 { 1 } else { -1 };
    let dir_y: i16 = if rng.next() % 2 == 0 { 1 } else { -1 };

    let color = pick_color(&mut rng);

    let mut s = BounceState {
        x: start_x,
        y: start_y,
        dx: dir_x,
        dy: dir_y,
        color,
        frame: 0,
    };

    backend
        .run_windowed(move |term: &mut Terminal<_>| {
            tick(term, &mut s);
            s.frame = s.frame.wrapping_add(1);

            if let Some(event) = term.poll(Duration::from_millis(16)) {
                match event {
                    Event::Key(k) if k.code == KeyCode::Escape => std::process::exit(0),
                    Event::Close => std::process::exit(0),
                    _ => {}
                }
            }
        })
        .expect("event loop failed");
}

struct Lcg {
    state: u64,
}

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    const fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state >> 33
    }
}

struct BounceState {
    x: u16,
    y: u16,
    dx: i16,
    dy: i16,
    color: Color,
    frame: u64,
}

fn tick(term: &mut Terminal<impl rg::Backend>, s: &mut BounceState) {
    let size = term.size();

    // ── Background tile grid on layer 0 (redrawn every frame) ──────────
    // A visible checkerboard of `#` and `.` that the @ glides over.
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

    // ── Every frame: clear layer 1 and redraw the @ ────────────────────
    term.layer(1);
    term.clear();

    let right = i64::from(size.width) - 1;
    let bottom = i64::from(size.height) - 1;

    #[allow(clippy::cast_possible_wrap)]
    let pos_x = i64::from(s.x) + i64::from(s.dx);
    #[allow(clippy::cast_possible_wrap)]
    let pos_y = i64::from(s.y) + i64::from(s.dy);

    if pos_x <= 0 || pos_x >= right {
        s.dx = -s.dx;
        s.color = pick_color_lcg(&mut Lcg::new(s.frame));
    }
    if pos_y <= 0 || pos_y >= bottom {
        s.dy = -s.dy;
        s.color = pick_color_lcg(&mut Lcg::new(s.frame.wrapping_add(1)));
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss
    )]
    {
        s.x = (i64::from(s.x) + i64::from(s.dx)).clamp(1, right - 1) as u16;
        s.y = (i64::from(s.y) + i64::from(s.dy)).clamp(1, bottom - 1) as u16;
    }

    // Sub-pixel offset: sawtooth -3 .. +3.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let sub_cycle = (s.frame as i64) % 14;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let off_x = ((sub_cycle - 6).abs()) as i16;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let off_y = {
        let c = (s.frame.wrapping_add(7) as i64) % 14;
        ((c - 6).abs()) as i16
    };

    term.fg(s.color);
    term.put_offset(s.x, s.y, off_x, off_y, '@');

    // ── Layer 2 header (redrawn every frame) ───────────────────────────
    term.layer(2);
    for x in 0..size.width {
        term.put_styled(
            x,
            0,
            ' ',
            rg::Style::new().bg(Color::Rgb {
                r: 30,
                g: 30,
                b: 45,
            }),
        );
    }
    let header = "rg DVD screensaver [Esc to quit]";
    let header_style = rg::Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
        r: 30,
        g: 30,
        b: 45,
    });
    #[allow(clippy::cast_possible_truncation)]
    let hx = size.width.saturating_sub(header.len() as u16) / 2;
    for (i, ch) in header.chars().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        term.put_styled(hx + i as u16, 0, ch, header_style);
    }

    // ── Every frame: layer 2 status footer ─────────────────────────────
    term.layer(2);
    let footer_bg = Color::Rgb {
        r: 30,
        g: 30,
        b: 45,
    };
    for x in 0..size.width {
        term.put_styled(x, size.height - 1, ' ', rg::Style::new().bg(footer_bg));
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

    term.present();
}

#[allow(clippy::missing_const_for_fn)]
fn pick_color_lcg(rng: &mut Lcg) -> Color {
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

#[allow(clippy::missing_const_for_fn)]
fn pick_color(rng: &mut Lcg) -> Color {
    pick_color_lcg(rng)
}
