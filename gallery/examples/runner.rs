//! Interactive gallery runner.
//!
//! 1. Pick an example.
//! 2. Pick a backend -- unsupported ones are greyed out.
//! 3. Launches `cargo run` with the correct feature flags.
//!
//! Deliberately minimal for now: the WASM option just shells out to `cargo run --target
//! wasm32-unknown-unknown` (relying on `.cargo/config.toml`'s `wasm-server-runner` runner) rather
//! than the fuller docs-parity preview (build + wasm-bindgen + serve + open browser)
//! `crates/examples/examples/runner.rs` does -- and there's no `--manifest` flag for CI yet,
//! either. Grow `EXAMPLES` and `Backend` here in lockstep as examples move over.
//!
//! Run with: `cargo run --example runner`

use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;

// ── Backend ───────────────────────────────────────────────────────────────

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
            Self::Headless => "no display -- prints a few frames to stdout",
        }
    }

    const fn features(self) -> &'static [&'static str] {
        match self {
            Self::Terminal => &["crossterm"],
            Self::Desktop | Self::Wasm => &["default-font"],
            // No feature: examples fall back to the Headless backend when
            // neither crossterm nor software is enabled.
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

// ── Example metadata ──────────────────────────────────────────────────────

struct Example {
    name: &'static str,
    description: &'static str,
    /// Backends this example supports.
    backends: &'static [Backend],
}

static EXAMPLES: &[Example] = &[Example {
    name: "01_hello_world",
    description: "smallest cross-backend App -- print, present, quit on input",
    backends: &[
        Backend::Terminal,
        Backend::Desktop,
        Backend::Wasm,
        Backend::Headless,
    ],
}];

// ── I/O helpers ─────────────────────────────────────────────────────────────

fn prompt(msg: &str) -> String {
    print!("{msg}");
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf).unwrap();
    buf.trim().to_owned()
}

// ── Launch ──────────────────────────────────────────────────────────────────

fn launch(ex: &Example, backend: Backend) -> ! {
    let features = backend.features();

    println!("\n  Launching: {}", ex.name);
    if !features.is_empty() {
        println!("  Features:  {}", features.join(","));
    }
    if let Some(target) = backend.target() {
        println!("  Target:    {target}");
    }
    println!();

    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--example", ex.name]);
    if !cfg!(debug_assertions) {
        cmd.arg("--release");
    }
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    if let Some(target) = backend.target() {
        cmd.args(["--target", target]);
    }

    let err = cmd.exec();
    eprintln!("  Failed to exec: {err}");
    std::process::exit(1);
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() {
    println!("retroglyph gallery\n");
    for (i, ex) in EXAMPLES.iter().enumerate() {
        println!("  {}) {} -- {}", i + 1, ex.name, ex.description);
    }

    let choice: usize = loop {
        let input = prompt(&format!("\nPick an example [1-{}]: ", EXAMPLES.len()));
        match input.parse::<usize>() {
            Ok(n) if n >= 1 && n <= EXAMPLES.len() => break n - 1,
            _ => println!("  Invalid choice."),
        }
    };
    let ex = &EXAMPLES[choice];

    println!();
    for (i, backend) in Backend::ALL.iter().enumerate() {
        let supported = ex.backends.contains(backend);
        let marker = if supported { " " } else { "x" };
        println!(
            "  {}) [{marker}] {} -- {}",
            i + 1,
            backend.label(),
            backend.summary()
        );
    }

    let backend = loop {
        let input = prompt(&format!("\nPick a backend [1-{}]: ", Backend::ALL.len()));
        match input.parse::<usize>() {
            Ok(n) if n >= 1 && n <= Backend::ALL.len() => {
                let backend = Backend::ALL[n - 1];
                if ex.backends.contains(&backend) {
                    break backend;
                }
                println!("  {} does not support {}.", ex.name, backend.label());
            }
            _ => println!("  Invalid choice."),
        }
    };

    launch(ex, backend);
}
