# TODO

## #12 — Sparse draw layer

**Background:** The current `Grid` is always dense — every cell is allocated and tracked regardless of whether it was drawn this frame. For roguelikes this is fine for the map layer, but bracket-lib's `SparseConsole` (which stores only explicitly-set cells) is cited as a key strength for entity layers and HUDs: you only pay for what you draw, and there's no need to clear and redraw the full grid each frame just to show a player glyph.

**Proposed design:** A `SparseGrid` or `DrawLayer` type that stores `(Position, Cell)` pairs rather than a flat buffer. During `Terminal::present`, it composes on top of the base grid by yielding its entries as the diff iterator, feeding them straight into the existing `Backend::draw` pipeline (which already consumes `Iterator<Item = (Position, &Cell)>`).

**Why it fits the current architecture:** `Backend::draw` is already backend-agnostic and iterator-driven. A sparse layer would implement the same diff interface without touching `Terminal` or any backend. Layering could be as simple as chaining two iterators.

**Considerations:**
- Decide whether `SparseGrid` owns a `HashMap<Position, Cell>` (fast lookup, heap) or a sorted `Vec<(Position, Cell)>` (cache-friendly scan, no hash).
- Wide-char continuation markers (`'\0'`) still need to be emitted for 2-wide glyphs placed in the sparse layer.
- Consider a `DrawLayer` trait so both `Grid` and `SparseGrid` can be composed uniformly.
- Compositing order and z-ordering are out of scope for the initial implementation; a simple "sparse over dense" model is enough for the common entity-on-map case.
