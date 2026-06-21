//! WASM-compatible software backend demo.
//!
//! Run with:
//!   `cargo run --target wasm32-unknown-unknown --example wasm_demo --features software-default-font`
//!
//! (Requires `wasm-server-runner` and a browser. Opens http://127.0.0.1:1334.)

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

mod util;

use rg::Pos;
use rg::backend::software::SoftwareBackendBuilder;

/// WASM entry point called by the browser.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    main();
    Ok(())
}

fn main() {
    let mut player = Pos::new(5, 5);

    let backend = SoftwareBackendBuilder::new()
        .title("rg WASM demo")
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
