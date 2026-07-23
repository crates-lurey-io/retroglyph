# Roadmap

Ideas surfaced by a final comparison pass over other terminal/roguelike/rendering libraries
(ratatui, bracket-lib, libtcod, python-tcod, doryen-rs, rot.js, notcurses, blessed, crossterm,
tcell, termbox2, ftxui, ebiten, xterm.js) before `docs/references/libs/` was deleted. Recorded here
so ideas aren't lost, and so the rejected ones aren't re-litigated later without a reason to revisit
them.

Near-term actionable items (the "Adopt (small)" rows below) were meant to get their own GitHub
issue, but this repository has issues disabled, so they're tracked here only, flagged as near-term.
Everything else stays narrative-only until it's actually being scheduled.

## Adopt

Medium effort, real capability gaps, not urgent. Nothing currently outstanding in this category;
items land here as they're identified, then move to a "Shipped" note below once implemented.

Shipped: subcell image-to-glyph blit (posterize a pixel block to the best block/quadrant/sextant
character, per doryen-rs/libtcod/notcurses) as `retroglyph_core::subcell`
(`quantize_half_block`/`quantize_quadrant`/`quantize_sextant`) -- see
`examples/examples/16_subcell_image.rs`.

Shipped: non-alternate-screen / inline rendering mode for the crossterm backend (termbox2, as a
documented gap) as `CrosstermOptions::alt_screen(false)` -- see `crates/crossterm/src/lib.rs`.

## Defer

Real ideas, not urgent, no current plan to schedule:

| Idea                                                                                             | Source                                              | Why deferred                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------ | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pipe-safe / non-TTY output degradation (auto-detect non-interactive stdout, strip control codes) | blessed (Python)                                    | Real gap, low urgency: retroglyph targets interactive games/dashboards, not CLI tools piped to files.                                                                   |
| REXPaint (`.xp`) file import                                                                     | libtcod, python-tcod, bracket-lib                   | Self-contained asset-format nicety with precedent, but no demonstrated user demand yet.                                                                                 |
| Sixel / Kitty graphics protocol output (real pixel images in a real terminal)                    | notcurses                                           | Valuable but high effort and fragmented terminal support (notcurses itself dropped iTerm2 support over this). The software backend already covers "I want real pixels." |
| Cell blend-mode enum (SCREEN, DODGE, BURN, OVERLAY, ...) beyond linear alpha                     | libtcod                                             | Niche VFX polish; `Grid::blit_alpha`'s linear blend already covers the common compositing case.                                                                         |
| Custom post-processing shaders (CRT scanlines, fog-of-war overlays) on the software backend      | ebiten (Kage), bracket-lib                          | Requires a GPU shader pipeline the softbuffer-based software backend doesn't have. Would need its own design doc before any code.                                       |
| Text-input / line-editor widget                                                                  | ftxui (`Input`), ratatui ecosystem (`tui-textarea`) | Scope question first: does retroglyph want to own interactive text entry? Not a clear miss given the free-function/no-retained-tree widget philosophy.                  |

## Reject

Recorded so these aren't re-proposed without new information:

| Idea                                                             | Source                                                | Why rejected                                                                                                                                                                                                          |
| ---------------------------------------------------------------- | ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| FOV algorithms (shadowcasting, ray/permissive/restrictive)       | libtcod, python-tcod, rot.js, bracket-lib, doryen-fov | Out of scope: retroglyph is a rendering/input library, not a roguelike toolkit.                                                                                                                                       |
| Pathfinding (A\*, Dijkstra maps)                                 | libtcod, python-tcod, rot.js, bracket-lib             | Same rationale as FOV.                                                                                                                                                                                                |
| Noise generation (Perlin/Simplex/Wavelet)                        | libtcod, bracket-lib                                  | Game-content generation, not rendering.                                                                                                                                                                               |
| BSP dungeon generation / map generators                          | libtcod, python-tcod, rot.js                          | Game logic, not rendering.                                                                                                                                                                                            |
| Name generation, dice-notation RNG                               | libtcod, bracket-lib                                  | Game content, unrelated to rendering/input.                                                                                                                                                                           |
| Multi-layer console with independent grid size/tileset per layer | bracket-lib                                           | Conflicts with retroglyph's single-grid-size-per-`Grid` model; would be a significant architectural change for a capability with no demonstrated use case.                                                            |
| Retained component tree with automatic focus-tree navigation     | ftxui                                                 | Conflicts with retroglyph's deliberate free-function/immediate-mode widget design. The equivalent problems (focus, hit-testing) are already solved without a retained tree via `FocusRing`/`HitTester`/`Interaction`. |
| Addon/plugin lifecycle system (`activate`/`dispose()`)           | xterm.js                                              | Doesn't fit a `no_std` core crate's minimal-surface design goals.                                                                                                                                                     |
| Full menu/dropdown/tabs/checkbox/radio widget set                | ftxui                                                 | `retroglyph-widgets` is deliberately a smaller "panels/gauges/tables/layout" set, not a full interactive form-widget framework.                                                                                       |
