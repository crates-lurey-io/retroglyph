//! Guards against the example metadata living in `examples/runner.rs` and
//! `.github/workflows/docs.yml` drifting apart.
//!
//! `runner.rs`'s `EXAMPLES` table is the source of truth for which examples
//! support the WASM backend and which feature group (`default-font`
//! vs `tilesets`) they need. `docs.yml`'s "Build WASM examples
//! (default-font)"/"(tilesets)" steps still hand-maintain their own bash
//! `--example` flag lists that must match (the Headless-demo build list,
//! Software packaging step, and examples-index generation all instead read
//! `runner.rs`'s data live at CI time via `cargo run --example runner --
//! --manifest`, so only these two build steps still risk drifting). Nothing
//! enforced that agreement, and it already drifted (see PR history) — this
//! test parses both files with light regexes and diffs the example sets so
//! future drift fails locally / in CI instead of silently shipping a demos
//! page missing an example.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Examples declared in `runner.rs`, split by whether they need
/// `tilesets` on top of `default-font` for the WASM backend.
struct RunnerWasmExamples {
    default_font: BTreeSet<String>,
    tilesets: BTreeSet<String>,
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Repo root: this crate lives at `crates/examples/`, so the workspace root
/// (which holds `.github/`) is two levels up from the manifest dir.
fn repo_root() -> PathBuf {
    manifest_dir().join("../..")
}

/// Extracts each `Example { ... }` block's `name`, `backends`, and
/// `backend_features` from `examples/runner.rs` well enough to answer
/// "does this example run on Wasm, and does it need tilesets".
///
/// This is a purpose-built scanner, not a Rust parser: it walks brace-matched
/// `Example { ... }` blocks and inspects their text with substring checks.
fn parse_runner_wasm_examples() -> RunnerWasmExamples {
    let src = fs::read_to_string(manifest_dir().join("examples/runner.rs"))
        .expect("failed to read examples/runner.rs");

    let mut default_font = BTreeSet::new();
    let mut tilesets = BTreeSet::new();

    for block in example_struct_blocks(&src) {
        if !block.contains("Backend::Wasm") {
            continue;
        }
        let name = extract_name(block);

        // An example needs tilesets on Wasm if either its shared
        // `extra_features` or its Wasm-specific `backend_features` override
        // mentions "tilesets".
        let wasm_override = extract_backend_override(block, "Backend::Wasm");
        let needs_tilesets = wasm_override
            .unwrap_or_else(|| extract_field(block, "extra_features"))
            .contains("tilesets");

        if needs_tilesets {
            tilesets.insert(name);
        } else {
            default_font.insert(name);
        }
    }

    RunnerWasmExamples {
        default_font,
        tilesets,
    }
}

/// Splits the `EXAMPLES` array body into individual `Example { ... }` block texts.
fn example_struct_blocks(src: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(start) = rest.find("Example {") {
        let brace_start = start + "Example {".len() - 1;
        let mut depth = 0i32;
        let mut end = brace_start;
        for (i, ch) in rest[brace_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = brace_start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        blocks.push(&rest[start..end]);
        rest = &rest[end..];
    }
    blocks
}

fn extract_name(block: &str) -> String {
    let after = block.split("name:").nth(1).expect("Example missing name");
    let quoted = after.split('"').nth(1).expect("name not quoted");
    quoted.to_owned()
}

/// Returns the raw source text of a top-level `field:` value up to the next
/// top-level field, e.g. `&[]` or `&["tilesets"]`.
fn extract_field<'a>(block: &'a str, field: &str) -> &'a str {
    let marker = format!("{field}:");
    let Some(after) = block.split_once(&marker).map(|(_, rest)| rest) else {
        return "";
    };
    // Field values here are single-line `&[...]` literals; take up to the
    // line's trailing comma.
    after.split(['\n']).next().unwrap_or("")
}

/// Looks for a `(Backend::X, &[...])` tuple inside `backend_features` for the
/// given backend variant and returns its feature-list text, if present.
fn extract_backend_override<'a>(block: &'a str, backend_variant: &str) -> Option<&'a str> {
    let backend_features = block.split_once("backend_features:")?.1;
    let mut rest = backend_features;
    loop {
        let start = rest.find(backend_variant)?;
        let after = &rest[start..];
        let comma = after.find(',')?;
        let close = after.find(')')?;
        if close < comma {
            rest = &after[close..];
            continue;
        }
        return Some(&after[comma..close]);
    }
}

/// Extracts the space-separated example name list following `for ex in` in
/// `docs.yml`'s "Generate examples index" step, which is the canonical list
/// of every WASM example the workflow expects to have built successfully.
fn parse_docs_yml_example_groups() -> (BTreeSet<String>, BTreeSet<String>) {
    let src = fs::read_to_string(repo_root().join(".github/workflows/docs.yml"))
        .expect("failed to read .github/workflows/docs.yml");

    let default_font = extract_yaml_example_flags(
        &src,
        "Build WASM examples (default-font)",
        "Build WASM examples (tilesets)",
    );
    let tilesets = extract_yaml_example_flags(
        &src,
        "Build WASM examples (tilesets)",
        "Build WASM Headless examples",
    );

    (default_font, tilesets)
}

/// Pulls every `--example <name>` occurrence between two step-name markers.
fn extract_yaml_example_flags(src: &str, start_marker: &str, end_marker: &str) -> BTreeSet<String> {
    let start = src.find(start_marker).unwrap_or_else(|| {
        panic!("docs.yml missing step {start_marker:?}; update this test's markers")
    });
    let end = src[start..]
        .find(end_marker)
        .map_or(src.len(), |rel| start + rel);
    let section = &src[start..end];

    section
        .split("--example")
        .skip(1)
        .map(|rest| rest.split_whitespace().next().unwrap_or("").to_owned())
        .collect()
}

#[test]
fn docs_yml_wasm_examples_match_runner_rs() {
    let runner = parse_runner_wasm_examples();
    let (docs_default_font, docs_tilesets) = parse_docs_yml_example_groups();

    assert_eq!(
        runner.default_font, docs_default_font,
        "\n\n.github/workflows/docs.yml's default-font WASM build list is out of \
         sync with examples/runner.rs. Update the `--example` list in the \
         \"Build WASM examples (default-font)\" step and the \"Package WASM \
         examples (Software)\" EXAMPLES array to match."
    );
    assert_eq!(
        runner.tilesets, docs_tilesets,
        "\n\n.github/workflows/docs.yml's tilesets WASM build list is out of sync \
         with examples/runner.rs. Update the `--example` list in the \
         \"Build WASM examples (tilesets)\" step and the \"Package WASM examples \
         (Software)\" EXAMPLES array to match."
    );
}
