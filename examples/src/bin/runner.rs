//! Interactive picker for retroglyph's examples: pick an example, pick a
//! backend, and this shells out to `cargo run --example <name> --features
//! <backend>` (compiling on demand) with stdio inherited.
//!
//! Deliberately dumb: no metadata file, no per-example capability list. Every
//! example in `examples/examples/*.rs` is assumed to support every backend
//! (by convention -- see `retroglyph_examples::Example`/`launch`), so the
//! example list is just "every `.rs` file in that directory," discovered at
//! runtime rather than hardcoded or read from `cargo metadata`.
//!
//! Each of the 3 backends has a WASM counterpart: press `w` at the backend
//! prompt to toggle between native and WASM before picking 1/2/3. A WASM
//! choice is built and packaged with the same real HTML/JS template the
//! docs site uses (via `tools/build-wasm-example.sh`), then served from a
//! throwaway local static server and opened in your default browser --
//! *not* run through `wasm-server-runner`, which only auto-invokes a
//! `#[wasm_bindgen(start)]` function and so only ever showed anything for
//! the Software variant; Headless and Terminal are driven by JS calling
//! specific exported functions in a loop, which only the real templates do.
//!
//! Run with `cargo run --bin runner` (or `--release` to also run the picked
//! native example in release mode; ignored for WASM choices, which always
//! build in release -- debug wasm32 binaries are large enough to make load
//! times painful for no benefit here).

use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// A backend choice offered by the picker: one label, the `--features`
/// value for the native variant (`None` means "run with no backend feature
/// enabled," i.e. the headless stdout fallback -- only ever true for the
/// native Headless entry), and the WASM variant name `tools/build-wasm-
/// example.sh` understands (`headless`/`terminal`/`software`).
struct Backend {
    label: &'static str,
    native_features: Option<&'static str>,
    wasm_variant: &'static str,
}

const BACKENDS: &[Backend] = &[
    Backend {
        label: "Headless",
        native_features: None,
        wasm_variant: "headless",
    },
    Backend {
        label: "Crossterm (real terminal) / Terminal (browser, xterm.js-style ANSI)",
        native_features: Some("crossterm"),
        wasm_variant: "terminal",
    },
    Backend {
        label: "Software (a window) / Software (browser, canvas)",
        native_features: Some("software"),
        wasm_variant: "software",
    },
];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is examples/; the workspace root is one level up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("examples/ has a parent directory")
        .to_path_buf()
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn list_examples() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(examples_dir())
        .expect("read examples/examples directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                return None;
            }
            path.file_stem()?.to_str().map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn prompt_choice(prompt: &str, options: &[String]) -> Option<usize> {
    println!("{prompt}");
    for (i, opt) in options.iter().enumerate() {
        println!("  {}) {opt}", i + 1);
    }
    print!("> ");
    std::io::stdout().flush().ok();

    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok()?;
    let choice: usize = line.trim().parse().ok()?;
    choice.checked_sub(1).filter(|&i| i < options.len())
}

/// Prompts for a backend (1/2/3), with `w` toggling native vs. WASM before
/// a numeric choice is entered. Returns the chosen backend index and whether
/// WASM was toggled on when the choice was made.
fn prompt_backend() -> Option<(usize, bool)> {
    let mut wasm = false;
    loop {
        println!(
            "Pick a backend{}:",
            if wasm {
                " [WASM -- 'w' to toggle back]"
            } else {
                " ('w' to toggle WASM)"
            }
        );
        for (i, backend) in BACKENDS.iter().enumerate() {
            println!("  {}) {}", i + 1, backend.label);
        }
        print!("> ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return None;
        }
        let trimmed = line.trim();

        if trimmed.eq_ignore_ascii_case("w") {
            wasm = !wasm;
            continue;
        }

        let choice: usize = trimmed.parse().ok()?;
        let idx = choice.checked_sub(1).filter(|&i| i < BACKENDS.len())?;
        return Some((idx, wasm));
    }
}

/// Builds and packages `example`'s `backend.wasm_variant` via
/// `tools/build-wasm-example.sh` into `target/wasm-preview/<example>/
/// <variant>/`, then serves that directory from a throwaway local server and
/// opens it in the default browser. Blocks until the user presses Enter.
fn run_wasm_preview(example: &str, backend: &Backend) -> ExitCode {
    let dest = workspace_root()
        .join("target/wasm-preview")
        .join(example)
        .join(backend.wasm_variant);

    println!("Building {example} ({}, WASM)...", backend.label);
    let status = std::process::Command::new("bash")
        .arg(workspace_root().join("tools/build-wasm-example.sh"))
        .arg(example)
        .arg(backend.wasm_variant)
        .arg(&dest)
        .status()
        .expect("failed to run tools/build-wasm-example.sh");

    if !status.success() {
        eprintln!("build failed");
        return ExitCode::FAILURE;
    }

    serve_and_open(&dest);
    ExitCode::SUCCESS
}

/// Serves `dir` on a throwaway local `TcpListener`, opens `http://127.0.0.1:
/// <port>/index.html` in the default browser, then blocks on stdin until the
/// user presses Enter. The server thread is not joined -- it's daemon-like
/// and harmless to leave running for the rest of this process's life (this
/// binary exits right after, which tears it down).
fn serve_and_open(dir: &Path) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local preview server");
    let port = listener.local_addr().expect("local addr").port();
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            serve_one(stream, &dir);
        }
    });

    let url = format!("http://127.0.0.1:{port}/index.html");
    println!("Serving preview at {url}");
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if std::process::Command::new(opener)
        .arg(&url)
        .status()
        .is_err()
    {
        println!("(couldn't auto-open a browser -- open {url} manually)");
    }

    println!("Press Enter once you're done previewing...");
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
}

/// Handles one HTTP request against `dir`: reads the request line, maps `/`
/// to `index.html`, and serves the file's bytes with a guessed
/// `Content-Type`. Rejects paths that would escape `dir` (e.g. `..`).
fn serve_one(mut stream: TcpStream, dir: &Path) {
    use std::io::BufRead as _;

    let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    // Drain the remaining request headers; we don't need them.
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
    }

    let Some(raw_path) = request_line.split_whitespace().nth(1) else {
        return;
    };
    let path = raw_path.split('?').next().unwrap_or("/");
    let rel = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    let Ok(dir_canonical) = dir.canonicalize() else {
        write_response(&mut stream, 500, "text/plain", b"server error");
        return;
    };
    match dir.join(rel).canonicalize() {
        Ok(file_path) if file_path.starts_with(&dir_canonical) => match std::fs::read(&file_path) {
            Ok(body) => write_response(&mut stream, 200, content_type_for(&file_path), &body),
            Err(_) => write_response(&mut stream, 404, "text/plain", b"not found"),
        },
        Ok(_) => write_response(&mut stream, 403, "text/plain", b"forbidden"),
        Err(_) => write_response(&mut stream, 404, "text/plain", b"not found"),
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("webmanifest" | "json") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let status_line = match status {
        200 => "200 OK",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        _ => "500 Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn main() -> ExitCode {
    let examples = list_examples();
    if examples.is_empty() {
        eprintln!("no examples found in {}", examples_dir().display());
        return ExitCode::FAILURE;
    }

    let Some(example_idx) = prompt_choice("Pick an example:", &examples) else {
        eprintln!("no example selected");
        return ExitCode::FAILURE;
    };
    let example = &examples[example_idx];

    let Some((backend_idx, wasm)) = prompt_backend() else {
        eprintln!("no backend selected");
        return ExitCode::FAILURE;
    };
    let backend = &BACKENDS[backend_idx];

    if wasm {
        return run_wasm_preview(example, backend);
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = env!("CARGO_MANIFEST_DIR");
    let mut args = vec![
        "run".to_owned(),
        "--manifest-path".to_owned(),
        format!("{manifest}/Cargo.toml"),
        "--example".to_owned(),
        example.clone(),
    ];
    if let Some(features) = backend.native_features {
        args.push("--features".to_owned());
        args.push(features.to_owned());
    }
    // Propagate this runner's own build profile: if you built/ran the
    // runner itself in release, run the picked example in release too.
    if !cfg!(debug_assertions) {
        args.push("--release".to_owned());
    }

    println!("$ {cargo} {}", args.join(" "));
    let status = std::process::Command::new(&cargo)
        .args(&args)
        .status()
        .expect("failed to run cargo");

    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
