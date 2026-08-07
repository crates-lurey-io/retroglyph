//! Headless smoke test for `examples/tutorial/01_hello.rs`.
//!
//! Deliberately lighter than the `examples/examples/*.rs` gallery's three-snapshot treatment
//! (`examples/AGENTS.md`): the tutorial's job is to compile and run headless for `docs/src/
//! tutorial/`'s `{{#include}}` anchors, not to join the WASM gallery or pin pixel-exact output.
//! This just asserts the example runs a few frames without panicking and draws `@`.

#![allow(unreachable_pub)]

#[path = "../tutorial/01_hello.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by this test
mod hello;

use hello::Hello;

#[test]
fn runs_headless_and_draws_the_player() {
    let views = retroglyph_examples::render_headless_frames::<Hello>(3);
    assert_eq!(views.len(), 3, "the example should not quit on its own");
    for view in &views {
        assert!(view.contains('@'), "frame did not draw the player:\n{view}");
    }
}
