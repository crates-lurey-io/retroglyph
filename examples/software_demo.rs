//! Playable software-backend demo matching the crossterm demo pattern.
//!
//! Run with:
//!   `cargo run --example software_demo --features software-default-font`

mod util;

use rg::backend::software::SoftwareBackendBuilder;

fn main() {
    let mut player = (5u16, 5u16);

    let backend = SoftwareBackendBuilder::new()
        .title("rg software demo")
        .grid_size(50, 25)
        .scale(2)
        .build()
        .expect("backend init failed (try the `software-default-font` feature)");

    backend
        .run_windowed(move |term| {
            if !util::game::tick(term, &mut player) {
                std::process::exit(0);
            }
        })
        .expect("event loop failed");
}
