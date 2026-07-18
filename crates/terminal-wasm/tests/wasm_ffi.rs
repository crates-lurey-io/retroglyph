//! Exercises `retroglyph_terminal_wasm::wasm`'s exported `wasm_terminal_*` functions under an
//! actual `wasm32` build, via `wasm-bindgen-test` (run through `wasm-pack test --node` -- see
//! `just test-wasm`). Everything in `src/lib.rs`'s own `#[cfg(test)]` module runs on the host
//! target and already covers `TerminalWasm`/`decode_key_event` directly; this file is the only
//! place that actually calls the `#[wasm_bindgen]`-exported functions themselves, since that
//! `wasm` module is `cfg(target_arch = "wasm32")` and doesn't exist in a host-target build at
//! all.
//!
//! Scope note: this FFI surface only exposes a handle registry
//! (`new`/`free`/`resize`/`push_key`/`push_mouse`/`push_paste`) plus `take_output`, which drains
//! whatever `TerminalRenderer` produced from a `Terminal::present()`
//! call -- but `present()` itself is never reachable through this FFI (by design: drawing is the
//! embedding Rust game's job, done directly against the `Terminal<TerminalWasm>` it owns; see
//! `src/lib.rs`'s "Usage from Rust" doc example). So `take_output` can only ever be observed as
//! empty from *this* test file -- there is no way to trigger a render without a `Terminal` the
//! test doesn't have access to through the public FFI. What's actually being verified here is the
//! registry's lifecycle and its robustness fixes from #131/#132: unique handles, and safe,
//! non-panicking no-ops for `resize`/`push_key`/`take_output` on a freed or never-issued handle
//! -- `push_paste` (#98) follows the exact same no-op-on-unknown-handle contract.
#![cfg(target_arch = "wasm32")]

use retroglyph_terminal_wasm::wasm::{
    wasm_terminal_free, wasm_terminal_new, wasm_terminal_push_key, wasm_terminal_push_mouse,
    wasm_terminal_push_paste, wasm_terminal_resize, wasm_terminal_take_output,
};
use retroglyph_terminal_wasm::{mouse_actions, mouse_buttons};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn new_handles_are_unique() {
    let a = wasm_terminal_new(10, 3);
    let b = wasm_terminal_new(10, 3);
    assert_ne!(a, b, "two live instances must not share a handle");
    wasm_terminal_free(a);
    wasm_terminal_free(b);
}

#[wasm_bindgen_test]
fn take_output_on_a_freshly_created_handle_is_empty() {
    let handle = wasm_terminal_new(10, 3);
    assert_eq!(wasm_terminal_take_output(handle), "");
    wasm_terminal_free(handle);
}

#[wasm_bindgen_test]
fn resize_and_push_key_on_a_freshly_created_handle_do_not_panic() {
    let handle = wasm_terminal_new(5, 2);
    wasm_terminal_resize(handle, 20, 10);
    wasm_terminal_push_key(handle, u32::from('a'), 0);
    wasm_terminal_free(handle);
}

#[wasm_bindgen_test]
fn push_mouse_on_a_freshly_created_handle_does_not_panic() {
    let handle = wasm_terminal_new(5, 2);
    wasm_terminal_push_mouse(handle, 1, 1, mouse_actions::DOWN, mouse_buttons::LEFT, 0);
    wasm_terminal_push_mouse(handle, 1, 1, mouse_actions::UP, mouse_buttons::LEFT, 0);
    wasm_terminal_free(handle);
}

#[wasm_bindgen_test]
fn push_paste_on_a_freshly_created_handle_does_not_panic() {
    let handle = wasm_terminal_new(5, 2);
    // Unlike `push_key`/`push_mouse`, there's no `decode_*` step to reject -- `text` is a plain
    // JS string, so the only thing to verify here is that it reaches a live handle without
    // panicking.
    wasm_terminal_push_paste(handle, "pasted text".to_string());
    wasm_terminal_free(handle);
}

#[wasm_bindgen_test]
fn operations_on_a_freed_handle_are_safe_no_ops() {
    let handle = wasm_terminal_new(4, 4);
    wasm_terminal_free(handle);

    // None of these should panic even though `handle` no longer exists (#131's robustness fix:
    // a warning is logged instead). Freeing twice is also a no-op, not a double-free/panic.
    wasm_terminal_resize(handle, 5, 5);
    wasm_terminal_push_key(handle, u32::from('z'), 0);
    wasm_terminal_push_mouse(handle, 0, 0, mouse_actions::MOVED, mouse_buttons::LEFT, 0);
    wasm_terminal_push_paste(handle, "ignored".to_string());
    assert_eq!(wasm_terminal_take_output(handle), "");
    wasm_terminal_free(handle);
}

#[wasm_bindgen_test]
fn operations_on_a_handle_that_was_never_issued_are_safe_no_ops() {
    let bogus_handle = 0xFFFF_FFFF;
    wasm_terminal_resize(bogus_handle, 1, 1);
    wasm_terminal_push_key(bogus_handle, u32::from('x'), 0);
    wasm_terminal_push_paste(bogus_handle, "ignored".to_string());
    wasm_terminal_push_mouse(
        bogus_handle,
        0,
        0,
        mouse_actions::MOVED,
        mouse_buttons::LEFT,
        0,
    );
    assert_eq!(wasm_terminal_take_output(bogus_handle), "");
}

#[wasm_bindgen_test]
fn push_key_with_an_undecodable_code_is_silently_dropped_not_a_panic() {
    let handle = wasm_terminal_new(10, 3);
    // 0xD800 is a lone UTF-16 surrogate half -- not a valid `char`, and below the named-key
    // range, so `decode_key_event` returns `None` and this must be a no-op, not a panic.
    wasm_terminal_push_key(handle, 0xD800, 0);
    wasm_terminal_free(handle);
}

#[wasm_bindgen_test]
fn push_mouse_with_an_unknown_action_is_silently_dropped_not_a_panic() {
    let handle = wasm_terminal_new(10, 3);
    // `0xFF` doesn't match any `mouse_actions` constant, so `decode_mouse_event` returns `None`
    // and this must be a no-op, not a panic.
    wasm_terminal_push_mouse(handle, 0, 0, 0xFF, mouse_buttons::LEFT, 0);
    wasm_terminal_free(handle);
}
