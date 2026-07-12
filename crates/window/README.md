# retroglyph-window

A shared windowing layer for [retroglyph](https://github.com/crates-lurey-io/retroglyph)'s
window-based backends (software today; GL/wgpu are future candidates). `Backend` fuses input and
output, which fits a terminal process but not a window, where an event loop owns input and a
renderer owns output separately -- this crate splits the two apart and reassembles them into one
`Backend` via `winit`.

Most consumers don't depend on this crate directly; use
[`retroglyph-software`](https://crates.io/crates/retroglyph-software) instead, which depends on it.
See [docs.rs](https://docs.rs/retroglyph-window) for the API.
