# retroglyph gallery

A gallery of numbered, single-concept examples, ordered from smallest to
most complete. Each example's doc comment names the specific
retroglyph/crates feature(s) it exists to show off, and how it differs from
its neighbors.

Browse interactively (pick an example, then a backend):

```sh
cargo run --example runner
```

Or run one directly:

```sh
cargo run --example 01_hello_world                          # Headless (prints a few frames)
cargo run --example 01_hello_world --features crossterm     # Terminal
cargo run --example 01_hello_world --features default-font  # Desktop window
cargo run --example 01_hello_world --features default-font --target wasm32-unknown-unknown  # WASM (browser tab)
```

The WASM run needs `wasm32-unknown-unknown` installed (`rustup target add
wasm32-unknown-unknown`) and `wasm-server-runner` on `PATH` (already wired up
as this workspace's `cfg(target_family = "wasm")` runner in
`.cargo/config.toml`); `cargo run` builds, serves, and opens the browser tab
for you.

Numbering is intentional, not incidental: examples are meant to be read in
order, each adding one new concept over the last. Later examples lean on
earlier ones freely instead of re-explaining fundamentals.
