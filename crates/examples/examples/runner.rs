//! Interactive example runner.
//!
//! 1. Pick an example.
//! 2. Pick a backend — unsupported ones are greyed out.
//! 3. Launches `cargo run` with the correct feature flags.
//!
//! Run with: `cargo run --example runner`

use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;

// ── ANSI helpers ──────────────────────────────────────────────────────────────

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

// ── Backend ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    Terminal,
    Desktop,
    Wasm,
    Headless,
}

impl Backend {
    const ALL: &'static [Self] = &[Self::Terminal, Self::Desktop, Self::Wasm, Self::Headless];

    const fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Desktop => "Desktop",
            Self::Wasm => "WASM",
            Self::Headless => "Headless",
        }
    }

    const fn summary(self) -> &'static str {
        match self {
            Self::Terminal => "runs in this shell",
            Self::Desktop => "native window",
            Self::Wasm => "browser tab (via wasm-server-runner)",
            Self::Headless => "no display — prints a few frames to stdout",
        }
    }

    const fn base_features(self) -> &'static [&'static str] {
        match self {
            Self::Terminal => &["crossterm"],
            Self::Desktop | Self::Wasm => &["default-font"],
            // Deliberately no backend feature: rg_run!/rg_run_software! fall
            // back to a Headless-backend main() when neither crossterm nor
            // software is enabled. See examples/util/mod.rs::run_headless.
            Self::Headless => &[],
        }
    }

    const fn target(self) -> Option<&'static str> {
        match self {
            Self::Wasm => Some("wasm32-unknown-unknown"),
            Self::Terminal | Self::Desktop | Self::Headless => None,
        }
    }
}

// ── Example metadata ──────────────────────────────────────────────────────────

struct Example {
    name: &'static str,
    description: &'static str,
    /// Backends this example supports. Empty = runs directly with no backend.
    backends: &'static [Backend],
    /// Feature flags required in addition to the backend's base features.
    extra_features: &'static [&'static str],
    /// Per-backend overrides: if a backend appears here, use these features
    /// instead of `extra_features` (still combined with the backend's base).
    /// Leave empty for the common case where all backends share the same extras.
    backend_features: &'static [(Backend, &'static [&'static str])],
    /// Whether this example has a working browser "Headless" demo wired up
    /// on the docs site (`--features wasm-headless`, see
    /// `util::wasm_headless_entry!`). Distinct from `Backend::Headless` above
    /// (that is the *local* stdout-printing CLI backend, always supported).
    /// Read by `.github/workflows/docs.yml` via this binary's `--manifest`
    /// flag -- this field is the single source of truth for which examples
    /// get a live "Headless" cell in the docs table instead of a greyed-out
    /// one. Keep this in sync as more examples get wasm-headless support.
    docs_headless: bool,
    /// Whether this example has a working browser "Terminal" (xterm.js) demo
    /// wired up on the docs site (`--features wasm-terminal`, see
    /// `util::wasm_terminal_entry!`). Same role as `docs_headless` but for
    /// the Terminal column -- read by `.github/workflows/docs.yml` via this
    /// binary's `--manifest` flag.
    docs_terminal: bool,
}

static EXAMPLES: &[Example] = &[
    Example {
        name: "dungeon_room",
        description: "interactive room — player, enemy, movement",
        backends: &[
            Backend::Terminal,
            Backend::Desktop,
            Backend::Wasm,
            Backend::Headless,
        ],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "sokoban",
        description: "Sokoban puzzle — push all boxes onto goals",
        backends: &[
            Backend::Terminal,
            Backend::Desktop,
            Backend::Wasm,
            Backend::Headless,
        ],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "roguelike_dungeon",
        description: "single-level roguelike — FoV, BFS pathfinding, layers",
        backends: &[
            Backend::Terminal,
            Backend::Desktop,
            Backend::Wasm,
            Backend::Headless,
        ],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "scrolling_roguelike",
        description: "scrolling roguelike — camera follow, charmap, shadowcast FoV",
        backends: &[
            Backend::Terminal,
            Backend::Desktop,
            Backend::Wasm,
            Backend::Headless,
        ],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "dashboard",
        description: "bashtop-style system monitor — gauges, sparklines, table",
        backends: &[Backend::Terminal, Backend::Desktop, Backend::Wasm],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "subpixel",
        description: "DVD-style bouncing @ with sub-pixel offsets",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "hex_battle",
        description: "hex battle replay — hex grid, units, sidebar, playback",
        backends: &[
            Backend::Terminal,
            Backend::Desktop,
            Backend::Wasm,
            Backend::Headless,
        ],
        // Terminal uses crossterm (ASCII art hexes).
        // Desktop/Wasm use tilesets for PNG hex sprites.
        // Headless uses neither (see Backend::base_features).
        extra_features: &[],
        backend_features: &[
            (Backend::Desktop, &["tilesets"]),
            (Backend::Wasm, &["tilesets"]),
        ],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "tileset",
        description: "custom PNG sprite sheets with alpha blending",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &["tilesets"],
        backend_features: &[],
        // `make_sprite_sheet()` calls `image::codecs::png::PngEncoder`
        // unconditionally whenever `tilesets` is enabled; `image` used to be
        // a dev-dependency only for `not(target_arch = "wasm32")`, which
        // made a plain `cargo build --target wasm32-unknown-unknown
        // --features tilesets` fail with no wasm-headless/wasm-terminal
        // involved at all. Fixed by adding a `cfg(target_arch = "wasm32")`
        // dev-dependency section for `image` in Cargo.toml (verified via
        // `wasm-bindgen --target nodejs` + node: both backends tick and
        // redraw without panicking).
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "sprite_stress",
        description: "alpha-blended sprite throughput benchmark",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &["tilesets"],
        backend_features: &[],
        // Same wasm32 `image` dev-dependency gap as `tileset` above, now
        // fixed the same way.
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "responsive_game_ui",
        description: "mobile-strategy kingdom map -- responsive layout, mouse/tap, transitions",
        backends: &[
            Backend::Terminal,
            Backend::Desktop,
            Backend::Wasm,
            Backend::Headless,
        ],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
    Example {
        name: "dirty_viz",
        description: "visualize which cells are redrawn each frame",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &[],
        backend_features: &[],
        docs_headless: true,
        docs_terminal: true,
    },
];

// The standalone `headless` backend demo lives at
// `crates/core/examples/headless.rs` now (it only depends on
// `retroglyph-core`, not this crate's shared game state), so it's no longer
// part of this picker. Run it with `cargo run -p retroglyph-core --example
// headless`.

// ── I/O helpers ───────────────────────────────────────────────────────────────

fn prompt(msg: &str) -> String {
    print!("{msg}");
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf).unwrap();
    buf.trim().to_owned()
}

fn combined_features(backend: Backend, ex: &Example) -> Vec<&'static str> {
    // Headless always runs with zero features: examples fall back to the
    // Headless backend precisely when neither `crossterm` nor `software` is
    // enabled (see rg_run!/rg_run_software! in examples/util/mod.rs). Mixing
    // in an example's usual extras (e.g. tilesets) would enable
    // `software` and route it to the normal windowed backend instead.
    if backend == Backend::Headless {
        return Vec::new();
    }

    let mut features: Vec<&'static str> = backend.base_features().to_vec();
    // Per-backend overrides take priority over the shared extra_features list.
    let extras = ex
        .backend_features
        .iter()
        .find(|(b, _)| *b == backend)
        .map_or(ex.extra_features, |(_, f)| f);
    for &f in extras {
        if !features.contains(&f) {
            features.push(f);
        }
    }
    features
}

// ── Launch ────────────────────────────────────────────────────────────────────

fn workspace_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent() // crates/
        .and_then(std::path::Path::parent) // workspace root
        .expect("crates/examples should be nested two levels under the workspace root")
        .to_path_buf()
}

fn launch(ex: &Example, backend: Option<Backend>) -> ! {
    let features: Vec<&str> = backend
        .map(|b| combined_features(b, ex))
        .unwrap_or_default();

    println!("\n  Launching: {}", ex.name);
    if !features.is_empty() {
        println!("  Features:  {}", features.join(","));
    }
    if let Some(target) = backend.and_then(Backend::target) {
        println!("  Target:    {target}");
    }
    println!();

    if backend == Some(Backend::Wasm) {
        launch_wasm(ex, &features);
    }

    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--example", ex.name]);
    // Propagate --release when the runner itself was built in release mode.
    if !cfg!(debug_assertions) {
        cmd.arg("--release");
    }
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    if let Some(target) = backend.and_then(Backend::target) {
        cmd.args(["--target", target]);
    }

    let err = cmd.exec();
    eprintln!("  Failed to exec: {err}");
    std::process::exit(1);
}

fn run_checked(mut cmd: Command, label: &str) {
    match cmd.status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("  {label} failed: {status}");
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("  Failed to run {label}: {e}");
            std::process::exit(1);
        }
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    let _ = Command::new(opener).arg(url).status();
}

/// Builds `ex` for `wasm32-unknown-unknown`, runs the artifact through
/// `wasm-bindgen --target web` (same invocation the docs site's CI uses),
/// and serves it wrapped in
/// `docs/templates/examples/software-template.html` — the exact template
/// GitHub Pages ships. This makes the local WASM preview match the deployed
/// demo pixel-for-pixel (full-screen canvas, mobile-web-app meta tags,
/// etc.), rather than diverging through wasm-server-runner's own
/// dev-only HTML shell (see `.cargo/config.toml`'s `runner`, still used by
/// plain `cargo run --target wasm32-unknown-unknown` for a quicker,
/// non-conforming smoke test).
fn launch_wasm(ex: &Example, features: &[&str]) -> ! {
    let root = workspace_root();
    let release = !cfg!(debug_assertions);
    let profile_dir = if release { "release" } else { "debug" };

    let mut build = Command::new("cargo");
    build.current_dir(&root);
    build.args([
        "build",
        "--example",
        ex.name,
        "--target",
        "wasm32-unknown-unknown",
    ]);
    if release {
        build.arg("--release");
    }
    if !features.is_empty() {
        build.args(["--features", &features.join(",")]);
    }
    run_checked(build, "cargo build");

    let wasm_path = root
        .join("target/wasm32-unknown-unknown")
        .join(profile_dir)
        .join("examples")
        .join(format!("{}.wasm", ex.name));

    let out_dir = root.join("target/wasm-preview").join(ex.name);
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("  Failed to create {}: {e}", out_dir.display());
        std::process::exit(1);
    }

    let mut bindgen = Command::new("cargo");
    bindgen.current_dir(&root);
    bindgen.args(["bin", "wasm-bindgen", "--target", "web", "--out-dir"]);
    bindgen.arg(&out_dir);
    bindgen.arg(&wasm_path);
    run_checked(bindgen, "wasm-bindgen");

    let template_path = root.join("docs/templates/examples/software-template.html");
    let template = match std::fs::read_to_string(&template_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  Failed to read {}: {e}", template_path.display());
            std::process::exit(1);
        }
    };
    if let Err(e) = std::fs::write(
        out_dir.join("index.html"),
        template.replace("__EXAMPLE__", ex.name),
    ) {
        eprintln!("  Failed to write index.html: {e}");
        std::process::exit(1);
    }

    // Bind an ephemeral port ourselves so the URL is known before the
    // server starts; the gap between dropping this listener and python
    // rebinding the same port is negligible for a local dev tool.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_or(8000, |a| a.port());
    let url = format!("http://127.0.0.1:{port}/");

    let mut server = match Command::new("python3")
        .current_dir(&out_dir)
        .args(["-m", "http.server", &port.to_string()])
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("  Built and packaged like the docs site at:");
            eprintln!("    {}", out_dir.display());
            eprintln!("  Failed to launch python3 to serve it ({e}) — serve it yourself, e.g.:");
            eprintln!("    npx serve {}", out_dir.display());
            std::process::exit(1);
        }
    };

    std::thread::sleep(std::time::Duration::from_millis(300));
    println!("  Serving:   {url}");
    open_browser(&url);

    let status = server.wait();
    std::process::exit(match status {
        Ok(s) => s.code().unwrap_or(0),
        Err(e) => {
            eprintln!("  Server error: {e}");
            1
        }
    });
}

// ── Main ──────────────────────────────────────────────────────────────────────

// ── Manifest (machine-readable, consumed by .github/workflows/docs.yml) ─────
//
// `EXAMPLES` above is the single source of truth for which examples support
// which backends. Rather than hand-duplicating that matrix a second time in
// docs.yml's bash heredocs, the docs workflow shells out to
// `cargo run --example runner -- --manifest` and parses this instead.
//
// Tab-separated, one example per line:
// `name\twasm_software\tdocs_headless\tdocs_terminal` where the three flag
// columns are `1`/`0`. `wasm_software` is `1` when the example builds for the
// existing canvas/software wasm backend (`Backend::Wasm` in the matrix
// above); `docs_headless`/`docs_terminal` mirror
// `Example::docs_headless`/`Example::docs_terminal`. No header row, so the
// shell side can loop over lines directly.
fn print_manifest() {
    for ex in EXAMPLES {
        let wasm_software = u8::from(ex.backends.contains(&Backend::Wasm));
        let docs_headless = u8::from(ex.docs_headless);
        let docs_terminal = u8::from(ex.docs_terminal);
        println!(
            "{}\t{wasm_software}\t{docs_headless}\t{docs_terminal}",
            ex.name
        );
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--manifest") {
        print_manifest();
        return;
    }

    'outer: loop {
        // ── Step 1: pick an example ──────────────────────────────────────
        println!("\n  rg example runner — pick an example:\n");
        for (i, ex) in EXAMPLES.iter().enumerate() {
            println!("  {:>2})  {:<28}  {}", i + 1, ex.name, ex.description);
        }
        println!("   q)  quit\n");

        let example_choice = prompt(&format!("  Choice (1-{}, q): ", EXAMPLES.len()));
        if example_choice == "q" {
            return;
        }
        let Some(ex) = example_choice
            .parse::<usize>()
            .ok()
            .filter(|&i| i >= 1 && i <= EXAMPLES.len())
            .map(|i| &EXAMPLES[i - 1])
        else {
            println!("  Invalid choice.");
            continue;
        };

        // No backend needed (e.g. headless) — launch immediately.
        if ex.backends.is_empty() {
            launch(ex, None);
        }

        // ── Step 2: pick a backend ───────────────────────────────────────
        loop {
            println!("\n  {} — pick a backend:\n", ex.name);

            // Build an ordered list of supported backends for numbering.
            let supported: Vec<Backend> = Backend::ALL
                .iter()
                .copied()
                .filter(|b| ex.backends.contains(b))
                .collect();

            let mut opt = 0usize;
            for &b in Backend::ALL {
                if ex.backends.contains(&b) {
                    opt += 1;
                    let features = combined_features(b, ex);
                    println!(
                        "  {:>2})  {:<10}  {}  [{}]",
                        opt,
                        b.label(),
                        b.summary(),
                        features.join(","),
                    );
                } else {
                    println!(
                        "{DIM}   -)  {:<10}  {}  (not supported){RESET}",
                        b.label(),
                        b.summary(),
                    );
                }
            }
            println!("   q)  back\n");

            let backend_choice = prompt(&format!("  Backend (1-{}, q): ", supported.len()));
            if backend_choice == "q" {
                continue 'outer;
            }
            let Some(&backend) = backend_choice
                .parse::<usize>()
                .ok()
                .filter(|&i| i >= 1 && i <= supported.len())
                .and_then(|i| supported.get(i - 1))
            else {
                println!("  Invalid choice.");
                continue;
            };

            launch(ex, Some(backend));
        }
    }
}
