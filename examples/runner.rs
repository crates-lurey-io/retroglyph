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
}

impl Backend {
    const ALL: &'static [Self] = &[Self::Terminal, Self::Desktop, Self::Wasm];

    const fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Desktop => "Desktop",
            Self::Wasm => "WASM",
        }
    }

    const fn summary(self) -> &'static str {
        match self {
            Self::Terminal => "runs in this shell",
            Self::Desktop => "native window",
            Self::Wasm => "browser tab (via wasm-server-runner)",
        }
    }

    const fn base_features(self) -> &'static [&'static str] {
        match self {
            Self::Terminal => &["crossterm"],
            Self::Desktop | Self::Wasm => &["software-default-font"],
        }
    }

    const fn target(self) -> Option<&'static str> {
        match self {
            Self::Wasm => Some("wasm32-unknown-unknown"),
            Self::Terminal | Self::Desktop => None,
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
}

static EXAMPLES: &[Example] = &[
    Example {
        name: "demo",
        description: "interactive room — player, enemy, movement",
        backends: &[Backend::Terminal, Backend::Desktop, Backend::Wasm],
        extra_features: &[],
    },
    Example {
        name: "software_subpixel_demo",
        description: "DVD-style bouncing @ with sub-pixel offsets",
        backends: &[Backend::Desktop, Backend::Wasm],
        extra_features: &[],
    },
    Example {
        name: "tileset_demo",
        description: "custom PNG sprite sheets with alpha blending",
        backends: &[Backend::Desktop, Backend::Wasm],
        extra_features: &["software-tilesets"],
    },
    Example {
        name: "headless_demo",
        description: "headless backend — no terminal or window needed",
        backends: &[],
        extra_features: &[],
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
    let mut features: Vec<&'static str> = backend.base_features().to_vec();
    for &f in ex.extra_features {
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

        // No backend needed (e.g. headless_demo) — launch immediately.
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
