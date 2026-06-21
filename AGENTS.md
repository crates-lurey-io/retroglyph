# AGENTS.md — Developer & Agent Guide

This guide details instructions for building, testing, linting, formatting, and navigating the `retroglyph`
codebase.

---

## Iterative Development Commands

Always use the [`Justfile`](Justfile) via `just` to automate development tasks.

### Build & Run

- **Check Compilation:** `cargo check --all-targets --all-features`
  - _Feature-gated check:_ `cargo check --features crossterm`
- **Clippy Lints:** `cargo clippy --all-targets --all-features` (Warnings are treated as errors)
- **Run Example:** `cargo run --example headless_demo`

### Formatting & Linting

- **Format all files (Rust + Markdown/JSON):** `just fmt`
- **Verify formatting without modifications:** `just fmt-check`
- **Check all project constraints:** `just check` (Runs formatting check, Clippy, tests, private

  rustdocs compilation, and `llms.txt` freshness check)

### Testing

- **Run all tests:** `just test` (Runs all unit and E2E tests with `--all-features`)
- **Run tests with verbose output:** `just test-v`
- **Run specific E2E test:** `cargo test --test e2e <test_name>`

### Feature Flags

- **`software-tilesets`** (implies `software`): Enables PNG sprite sheet tilesets and sprite

  caching for the software backend. Pulls in `image` (PNG decoding), `alpha-blend` (RGBA blending),
  and `log` (collision warnings) as optional dependencies.

### Logging

- Use the `log` crate (feature-gated) for non-critical warnings (e.g., codepoint collision in
  tilesets) AND for fatal-but-actionable errors (e.g., window/surface init failures).  Do NOT use
  `eprintln!` anywhere inside the library — `log` lets applications control where output goes.
- Fatal init errors that prevent the backend from starting should use `log::error!` + graceful
  shutdown (`event_loop.exit()`), not `panic!` or `eprintln!`.

### Alpha Blending

- Use `alpha-blend` (Apache 2.0/MIT) for RGBA compositing types like `U8x4Rgba`. See

  `.matan/alpha-blend.md` for the full audit.

- **Note:** `U8x4Rgba::source_over` had an alpha computation bug in `0.2.0` (`a*a` in place of

  `a*255`). This was fixed in `0.2.1`. Always depend on `>=0.2.1`.

### Documentation & Summaries

- **Generate private rustdocs:** `just doc`
- **Generate LLM text summaries (`llms.txt` & `llms-full.txt`):** `just llms`
- **Verify LLM summary freshness:** `just llms-check`

---

## Project Documentation Directory Structure

All domain references, architectural designs, and coding standards are stored in the
[`docs/`](docs/) directory:

- **[`docs/design/`](docs/design/) (ADRs and Implementation Plans):**
  - Contains Architectural Decision Records specifying the layout and boundaries of the crate.
  - Contains step-by-step milestone tracking documents (e.g.,

    [`002-foundations-plan.md`](docs/design/002-foundations-plan.md) and
    [`003-crossterm-backend.md`](docs/design/003-crossterm-backend.md)).

  - **Rule:** Before starting a new feature or milestone, consult the corresponding plan in this

    directory to match the specified design, structures, and behavior exactly.

- **[`docs/references/`](docs/references/) (Deep-Dive Domain Knowledge):**
  - `backends/`: Technical specifications for other visual/non-visual backends (Canvas, DRM, SDL,

    OpenGL, WebGL, etc.).

  - `core/`: Deep-dives into roguelike game development systems, Unicode/text handling, font

    rendering, testing strategies, and the crate's threading/concurrency model.

  - `libs/`: Analytical references comparing other libraries (Bracket-lib, Ratatui, Rot-js, Libtcod,

    etc.) to inform our design choices.

- **[`docs/style/`](docs/style/) (Development Guidelines):**
  - Guides on Rust API design guidelines, performance books, and best practices from various leading

    Rust teams.

  - **Rule:** Any new module or feature added to the crate must adhere to the core guidelines

    defined in the `docs/style/` reference documents.
