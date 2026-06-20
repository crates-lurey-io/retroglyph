//! Software-backend demo showing layers, sub-cell offsets, and compositing.
//!
//! Renders a multi-layer scene: a background pattern on layer 0, a bouncing
//! character on layer 1, and a text overlay on layer 2.
//!
//! Run with:
//!   `cargo run --example software_demo --features software-default-font`

use rg::backend::software::SoftwareBackendBuilder;
use rg::event::{Event, KeyCode};
use rg::style::Style;
use rg::{Color, Terminal};
use std::time::Duration;

fn main() {
    let backend = SoftwareBackendBuilder::new()
        .title("rg layers demo")
        .grid_size(50, 20)
        .scale(2)
        .build()
        .expect("backend init failed (try the `software-default-font` feature)");

    // Frame counter for animation.
    let mut frame = 0u64;

    backend
        .run(move |term: &mut Terminal<_>| {
            draw(term, frame);
            frame = frame.wrapping_add(1);

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

fn draw(term: &mut Terminal<impl rg::Backend>, frame: u64) {
    let size = term.size();

    // ── Frame 0: draw the static background pattern on layer 0 ──────────
    if frame == 0 {
        term.layer(0);
        // Fill the grid with a checkerboard-like dot pattern.
        for y in 0..size.height {
            for x in 0..size.width {
                let is_dot = (x + y) % 4 == 0;
                if is_dot {
                    term.fg(Color::Rgb {
                        r: 60,
                        g: 60,
                        b: 80,
                    });
                    term.put(x, y, ':');
                } else {
                    term.fg(Color::Rgb {
                        r: 20,
                        g: 20,
                        b: 30,
                    });
                    term.put(x, y, '.');
                }
            }
        }
    }

    // ── Every frame: draw a bouncing character on layer 1 ───────────────
    // Clear layer 1 so the old position disappears without affecting layer 0.
    term.layer(1);
    term.clear();

    // Bounce: dx oscillates between -2 and +2, dy oscillates between -1 and +1.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let dx = ((frame as i64 % 20).abs() - 10) as i16;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let dy = ((frame as i64 % 12).abs() - 6) as i16;
    let cx = size.width / 2;
    let cy = size.height / 2;

    term.fg(Color::BRIGHT_GREEN);
    term.put_offset(cx, cy, dx, dy, '@');

    // ── Frame 0: draw a static text header on layer 2 ───────────────────
    if frame == 0 {
        term.layer(2);
        let header = "rg layers demo [Esc to quit]";
        let style = Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
            r: 40,
            g: 40,
            b: 60,
        });

        // Center the header at the top.
        #[allow(clippy::cast_possible_truncation)]
        let hx = size.width.saturating_sub(header.len() as u16) / 2;
        for (i, ch) in header.chars().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            term.put_styled(hx + i as u16, 0, ch, style);
        }

        // Footer on layer 2 at the bottom.
        let footer = format!("layer 0: dots  |  layer 1: @ (offset {dx},{dy})  |  layer 2: text");
        #[allow(clippy::cast_possible_truncation)]
        let fx = size.width.saturating_sub(footer.len() as u16) / 2;
        term.bg(Color::Rgb {
            r: 40,
            g: 40,
            b: 60,
        });
        term.fg(Color::BRIGHT_BLACK);
        term.print(fx, size.height - 1, &footer);
    }

    term.present();
}
