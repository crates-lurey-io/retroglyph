# Run in a browser

Every backend compiles to `wasm32-unknown-unknown`. Which one to use depends on what the game looks
like in the browser: a text UI inside a terminal emulator widget, or a canvas.

## Text UI: `terminal-wasm`

[`retroglyph-terminal-wasm`](https://docs.rs/retroglyph-terminal-wasm) implements `Backend`
directly, like `Headless`: there's no event loop in this crate at all. A browser terminal emulator
(xterm.js, or any other; the crate has no dependency on one) is driven from JS, which calls in once
per animation frame to pull freshly rendered ANSI bytes and push back whatever input it collected.
On `wasm32` the crate exposes free functions (`wasm_terminal_new`, `wasm_terminal_resize`,
`wasm_terminal_push_key`, `wasm_terminal_take_output`, plus mouse/paste/focus variants) that drive a
`TerminalWasm` by opaque handle. Here's the crate's own reference driver for xterm.js in full:

```js
{{#include ../../../crates/terminal-wasm/js/xterm-driver.js}}
```

That's a wiring template, not a full game: it plumbs input/output through the FFI but calls no
per-frame drawing logic of its own; that's still your Rust code, holding a `Terminal<TerminalWasm>`
the same way it would hold a `Terminal<Headless>` in a test.

A game built on `retroglyph-core`'s `App` trait usually wants this crate's `app_entry!` macro
instead of driving `TerminalWasm` by hand: it generates a single-instance-per-page FFI surface that
owns the `Terminal<TerminalWasm>` and drives `App::update` for you, including a backgrounded-tab
delta clamp. See [docs.rs](https://docs.rs/retroglyph-terminal-wasm) for both.

## Canvas: `software`, `gl`, or `wgpu`

All three windowed backends port to `wasm32` unchanged via `winit`'s web backend: the same
`run_windowed`/`run_app` call that opens a native window targets a `<canvas>` element in the browser
instead, with `gl` speaking WebGL2 and `wgpu` speaking WebGPU on that target. See
[Choose a backend](./choose-a-backend.md) for which of the three to reach for. One behavioral
difference to know about porting from native: on `wasm32` the browser owns frame pacing (`winit`
services each requested redraw on the next `requestAnimationFrame`), so `WindowConfig::fit`'s
`target_fps` cap is a native-only optimization; an app that relies on it to throttle below the
display refresh rate will run uncapped once it's running in a browser tab.

## Building and packaging

Add the `wasm32-unknown-unknown` target and `wasm-bindgen`, then build your binary/example the same
way you would natively, aimed at that target:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
cargo build --target wasm32-unknown-unknown --release --features software  # or gl, wgpu, terminal-wasm
wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/your_game.wasm
```

`wasm-bindgen`'s output is an ES module (`pkg/your_game.js`) plus the `.wasm` binary; serve them
over real HTTP (`fetch()`-ing a `.wasm` module is blocked from a `file://` origin) alongside an HTML
page that calls the generated `init()` before anything else. `tools/build-wasm-example.sh` in this
workspace is a complete, working reference for exactly this build-and-package step, used to produce
the [live examples gallery](https://main.retroglyph.dev/examples/): every example in this repo runs
there in all four wasm-capable variants (headless text, `terminal-wasm`, `software` canvas, `gl`
WebGL2) with no local toolchain required to try one.

## See also

- [Choose a backend](./choose-a-backend.md) for the terminal-vs-canvas decision in full.
- [Handle resize](./handle-resize.md): a browser window/canvas resizes the same way a native one
  does, through `Event::Resize`.
