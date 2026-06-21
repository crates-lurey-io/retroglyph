//! Interactive example runner.
//!
//! Reads Cargo.toml for `[[example]]` entries and lets you pick one to run
//! with its required features.
//!
//! Run with:
//!   `cargo run --example runner`

use serde::Deserialize;
use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;

#[derive(Deserialize)]
struct CargoToml {
    #[serde(rename = "example")]
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    name: String,
    #[serde(default)]
    #[serde(rename = "required-features")]
    required_features: Vec<String>,
}

fn main() {
    // Read and parse Cargo.toml
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| String::from("."));
    let cargo_toml_path = format!("{manifest}/Cargo.toml");
    let text = std::fs::read_to_string(&cargo_toml_path).expect("failed to read Cargo.toml");
    let parsed: CargoToml = toml::from_str(&text).expect("failed to parse Cargo.toml");

    // Show examples (skip the runner itself)
    let examples: Vec<&Example> = parsed
        .examples
        .iter()
        .filter(|e| e.name != "runner")
        .collect();

    loop {
        println!("\n  rg example runner — pick one:\n");

        for (i, ex) in examples.iter().enumerate() {
            let features = if ex.required_features.is_empty() {
                String::new()
            } else {
                format!(" [{}]", ex.required_features.join(", "))
            };
            let wasm = if ex.name.contains("wasm") {
                " (WASM)"
            } else {
                ""
            };
            println!("  {:>2}) {:<30}{}{}", i + 1, ex.name, features, wasm);
        }

        println!("  {:>2}) quit\n", 'q');

        print!("  Run (1-{}, q): ", examples.len());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).unwrap();
        let input = input.trim();

        if input == "q" {
            return;
        }

        let index: usize = match input.parse::<usize>() {
            Ok(i) if i >= 1 && i <= examples.len() => i - 1,
            _ => {
                println!("  Invalid choice");
                continue;
            }
        };

        let ex = examples[index];
        let wasm = ex.name.contains("wasm");

        println!("\n  Launching: {} (WASM: {})", ex.name, wasm);
        println!("  Features:  {}", ex.required_features.join(", "));
        println!();

        let mut cmd = Command::new("cargo");
        cmd.arg("run");
        cmd.arg("--example");
        cmd.arg(&ex.name);

        if !ex.required_features.is_empty() {
            cmd.arg("--features");
            cmd.arg(ex.required_features.join(","));
        }

        if wasm {
            cmd.arg("--target");
            cmd.arg("wasm32-unknown-unknown");
        }

        // Replace the runner process with the example.
        // When the example exits, control returns to the shell.
        let err = cmd.exec();
        eprintln!("  Failed to exec: {err}");
        return;
    }
}
