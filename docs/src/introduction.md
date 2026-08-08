# Introduction

retroglyph is a 2D pseudographic terminal library for Rust: a double-buffered `Terminal`, styled
cells, text/layout helpers, and input events, generic over a pluggable `Backend` (a real terminal
via `crossterm`, or a native window / browser tab via `software`, `gl`, or `wgpu`). See the
[API documentation](../crates/index.html) for the full reference.

This book is for walkthroughs and explanations that don't fit a doc comment. Code blocks embedded
here are pulled out of the workspace's own compiled examples with mdBook's `{{#include}}`, using
named anchors rather than copy-pasted snippets, so a renamed or removed API breaks this book's build
(`just book`, part of `just doc` and `just check`) instead of leaving stale prose behind unnoticed.
For example, the draw step from `01_hello_world`:

```rust,ignore
{{#include ../../examples/examples/01_hello_world.rs:draw}}
```

See the sidebar for the tutorial (a walked-through game, chapter by chapter), how-to pages (one task
each), and explanation pages (background on how a piece of retroglyph is designed).
