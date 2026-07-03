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
    },
    Example {
        name: "dashboard",
        description: "bashtop-style system monitor — gauges, sparklines, table",
        backends: &[Backend::Terminal, Backend::Desktop, Backend::Wasm],
        extra_features: &[],
        backend_features: &[],
    },
    Example {
        name: "subpixel",
        description: "DVD-style bouncing @ with sub-pixel offsets",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &[],
        backend_features: &[],
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
    },
    Example {
        name: "tileset",
        description: "custom PNG sprite sheets with alpha blending",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &["tilesets"],
        backend_features: &[],
    },
    Example {
        name: "sprite_stress",
        description: "alpha-blended sprite throughput benchmark",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &["tilesets"],
        backend_features: &[],
    },
    Example {
        name: "dirty_viz",
        description: "visualize which cells are redrawn each frame",
        backends: &[Backend::Desktop, Backend::Wasm, Backend::Headless],
        extra_features: &[],
        backend_features: &[],
    },
    Example {
        name: "headless",
        description: "headless backend — no terminal or window needed",
        backends: &[],
        extra_features: &[],
        backend_features: &[],
    },
];

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

fn wasm_runner_path() -> std::path::PathBuf {
    // Path mirrors the runner entry in .cargo/config.toml.
    let manifest = env!("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest).join("bin/bin/wasm-server-runner")
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

    if backend == Some(Backend::Wasm) {
        let runner = wasm_runner_path();
        if !runner.exists() {
            eprintln!("\n  wasm-server-runner not found at {}", runner.display());
            eprintln!("  Run `just setup-wasm` to install it, then try again.");
            std::process::exit(1);
        }
    }

    println!();

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

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
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
