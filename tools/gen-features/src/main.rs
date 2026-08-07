//! Generates each crate's Features doc section (in `src/lib.rs` and `README.md`) from the
//! comments already sitting above each key in that crate's `Cargo.toml` `[features]` table.
//!
//! Run via `cargo gen-features` (writes) or `cargo gen-features --check` (verifies without
//! writing; used by `just check-features`). See `crates/*/Cargo.toml` for the comment
//! convention: whatever `#` comment lines sit directly above a feature key become that
//! feature's doc text, rendered into the `<!-- gen-features:start -->`/`<!-- gen-features:end
//! -->` marker region already present in that crate's `src/lib.rs` and `README.md`.

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::OnceLock,
};

use regex::Regex;

const START_MARKER: &str = "<!-- gen-features:start -->";
const END_MARKER: &str = "<!-- gen-features:end -->";

struct Feature {
    name: String,
    is_default: bool,
    doc: Vec<String>,
}

#[derive(Clone, Copy)]
enum Flavor {
    /// `src/lib.rs`: keeps rustdoc intra-doc links, since rustdoc resolves them itself.
    Rustdoc,
    /// `README.md`: plain Markdown can't resolve intra-doc links, so they're stripped to code spans.
    Readme,
}

fn main() -> ExitCode {
    let check = env::args().skip(1).any(|arg| arg == "--check");
    let crates_dir = workspace_root().join("crates");

    let mut crate_dirs: Vec<PathBuf> = match fs::read_dir(&crates_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        Err(err) => {
            eprintln!("error: reading {}: {err}", crates_dir.display());
            return ExitCode::FAILURE;
        }
    };
    crate_dirs.sort();

    let mut touched = Vec::new();
    let mut failed = false;

    for crate_dir in crate_dirs {
        match process_crate(&crate_dir, check) {
            Ok(paths) => touched.extend(paths),
            Err(err) => {
                eprintln!("error: {err}");
                failed = true;
            }
        }
    }

    if failed {
        return ExitCode::FAILURE;
    }

    if check {
        for path in &touched {
            eprintln!(
                "stale feature docs: {} (run `just gen-features`)",
                path.display()
            );
        }
        if touched.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    } else {
        for path in &touched {
            println!("updated {}", path.display());
        }
        ExitCode::SUCCESS
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("tools/gen-features is always two directories below the workspace root")
}

/// Processes one `crates/*` member. Returns the paths that are stale (`--check`) or were
/// rewritten (default mode); an empty vec means both files already matched.
fn process_crate(crate_dir: &Path, check: bool) -> Result<Vec<PathBuf>, String> {
    let cargo_toml_path = crate_dir.join("Cargo.toml");
    let src = fs::read_to_string(&cargo_toml_path)
        .map_err(|err| format!("reading {}: {err}", cargo_toml_path.display()))?;

    let features = parse_features(&src)
        .map_err(|err| format!("parsing {}: {err}", cargo_toml_path.display()))?;
    if features.is_empty() {
        return Ok(Vec::new());
    }

    let rustdoc_body = render(&features, Flavor::Rustdoc);
    let readme_body = render(&features, Flavor::Readme);

    let mut changed = Vec::new();
    if update_markers(&crate_dir.join("src/lib.rs"), &rustdoc_body, "//! ", check)? {
        changed.push(crate_dir.join("src/lib.rs"));
    }
    if update_markers(&crate_dir.join("README.md"), &readme_body, "", check)? {
        changed.push(crate_dir.join("README.md"));
    }
    Ok(changed)
}

fn parse_features(src: &str) -> Result<Vec<Feature>, String> {
    let doc = toml_edit::Document::parse(src).map_err(|err| err.to_string())?;

    let Some(features_item) = doc.get("features") else {
        return Ok(Vec::new());
    };
    let Some(table) = features_item.as_table_like() else {
        return Ok(Vec::new());
    };

    let mut defaults = HashSet::new();
    if let Some(default_item) = table.get("default")
        && let Some(array) = default_item.as_array()
    {
        for value in array {
            if let Some(name) = value.as_str() {
                defaults.insert(name.to_string());
            }
        }
    }

    let mut features = Vec::new();
    for (key_path, _value) in table.get_values() {
        let key = key_path[0];
        let name = key.get();
        if name == "default" {
            continue;
        }
        // `__`-prefixed features are an internal-only convention: never meant to be enabled by a
        // consumer, so they have no business in a consumer-facing feature table. No crate here
        // currently has one; the convention outlives any particular use of it, and keeping the
        // support means the next one costs nothing. Skipping them here means the convention is
        // enough on its own -- nobody has to remember to also keep such a feature's doc comment
        // out of this generator's input, or notice it leaking into `lib.rs`/`README.md` in review.
        if name.starts_with("__") {
            continue;
        }

        // The raw text (comments, blank lines) between the previous item and this key.
        let prefix = key.leaf_decor().prefix().map_or("", |raw| {
            raw.as_str()
                .unwrap_or_else(|| raw.span().map_or("", |span| &doc.raw()[span]))
        });

        features.push(Feature {
            is_default: defaults.contains(name),
            doc: trailing_comment_block(prefix),
            name: name.to_string(),
        });
    }

    features.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(features)
}

/// Given the raw text preceding a feature key, returns the contiguous `#`-comment lines
/// immediately above it. Scans backward from the key so an earlier feature's own trailing
/// blank line (or unrelated commentary further up) doesn't get pulled in.
fn trailing_comment_block(prefix: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in prefix.lines().rev() {
        let Some(comment) = line.trim().strip_prefix('#') else {
            break;
        };
        lines.push(
            comment
                .strip_prefix(' ')
                .unwrap_or(comment)
                .trim_end()
                .to_string(),
        );
    }
    lines.reverse();
    lines
}

fn render(features: &[Feature], flavor: Flavor) -> String {
    let defaults: Vec<&str> = features
        .iter()
        .filter(|feature| feature.is_default)
        .map(|feature| feature.name.as_str())
        .collect();

    // `summary` is the always-visible line: short enough to sit inside a `<summary>` on
    // `README.md`, and reused verbatim as the intro line on rustdoc, where there's no
    // `<summary>` length constraint. `long_intro` is the fuller no-defaults sentence, too long
    // for a `<summary>`; it stands alone on rustdoc and moves into the `<details>` body on
    // `README.md`.
    let (summary, long_intro) = if defaults.is_empty() {
        (
            "Features: all optional, none enabled by default.".to_string(),
            Some(
                "This crate has no default features; every feature below is optional and off unless enabled.\n"
                    .to_string(),
            ),
        )
    } else {
        let labels: Vec<String> = defaults.iter().map(|name| format!("`{name}`")).collect();
        (format!("Default features: {}.", labels.join(", ")), None)
    };

    let mut features_block = String::new();
    for feature in features {
        features_block.push_str("\n### `");
        features_block.push_str(&feature.name);
        features_block.push_str("`\n\n");
        features_block.push_str(if feature.is_default {
            "🟢 Enabled by default.\n"
        } else {
            "⚪ Optional.\n"
        });
        features_block.push('\n');
        for line in &feature.doc {
            let rendered = match flavor {
                Flavor::Rustdoc => line.clone(),
                Flavor::Readme => strip_doc_links(line),
            };
            features_block.push_str(&rendered);
            features_block.push('\n');
        }
    }

    match flavor {
        Flavor::Rustdoc => {
            let mut out = String::new();
            if let Some(intro) = &long_intro {
                out.push_str(intro);
            } else {
                out.push_str(&summary);
                out.push('\n');
            }
            out.push_str(&features_block);
            out
        }
        // Wrapped in `<details>`/`<summary>` so the feature reference doesn't dominate the
        // README a new user lands on (retroglyph#1251). The blank line after `<summary>...
        // </summary>` and the one before `</details>` are load-bearing: per CommonMark, an HTML
        // block only ends at a blank line, so without them the `###` headings inside render as
        // literal text instead of Markdown. Matches the house style set by the root README's
        // table-of-contents `<details>`.
        Flavor::Readme => {
            let mut out = String::new();
            out.push_str("<details>\n\n<summary>");
            out.push_str(&summary);
            out.push_str("</summary>\n");
            if let Some(intro) = &long_intro {
                out.push('\n');
                out.push_str(intro);
            }
            out.push_str(&features_block);
            out.push_str("\n</details>\n");
            out
        }
    }
}

/// Turns rustdoc intra-doc link markup into plain code spans: `` [`Foo::bar`](path) `` and the
/// shortcut form `` [`Foo`] `` both become `` `Foo` ``/`` `Foo::bar` ``, matching how README.md
/// is already hand-written today.
fn strip_doc_links(line: &str) -> String {
    static WITH_PATH: OnceLock<Regex> = OnceLock::new();
    static SHORTCUT: OnceLock<Regex> = OnceLock::new();

    let with_path =
        WITH_PATH.get_or_init(|| Regex::new(r"\[`([^`]+)`\]\([^)]*\)").expect("valid regex"));
    let shortcut = SHORTCUT.get_or_init(|| Regex::new(r"\[`([^`]+)`\]").expect("valid regex"));

    let step1 = with_path.replace_all(line, "`$1`");
    shortcut.replace_all(&step1, "`$1`").into_owned()
}

/// Replaces the text between `START_MARKER`/`END_MARKER` in `path` with `body`, each line
/// prefixed by `prefix` (`"//! "` for `lib.rs`, `""` for `README.md`). Returns whether the
/// file's generated region changed; in `--check` mode nothing is written.
///
/// Drift is judged on whitespace-normalized content, not exact bytes: `README.md` prose gets
/// reflowed by `prettier`'s `proseWrap` right after this tool runs (see the `gen-features`
/// Justfile recipe), so a line-wrap difference alone is not stale content -- treating it as one
/// would make this tool and `prettier` fight over the same lines on every run.
fn update_markers(path: &Path, body: &str, prefix: &str, check: bool) -> Result<bool, String> {
    let original =
        fs::read_to_string(path).map_err(|err| format!("reading {}: {err}", path.display()))?;

    let start_line = format!("{prefix}{START_MARKER}");
    let end_line = format!("{prefix}{END_MARKER}");

    let start = original
        .find(&start_line)
        .ok_or_else(|| format!("{}: missing `{start_line}`", path.display()))?;
    let start_end = start + start_line.len();
    let end = original[start_end..]
        .find(&end_line)
        .map(|offset| start_end + offset)
        .ok_or_else(|| {
            format!(
                "{}: missing `{end_line}` after start marker",
                path.display()
            )
        })?;

    let current_region = &original[start_end..end];

    let mut new_region = String::new();
    new_region.push('\n');
    for line in body.lines() {
        if line.is_empty() {
            new_region.push_str(prefix.trim_end());
        } else {
            new_region.push_str(prefix);
            new_region.push_str(line);
        }
        new_region.push('\n');
    }

    if normalize_whitespace(current_region) == normalize_whitespace(&new_region) {
        return Ok(false);
    }
    if check {
        return Ok(true);
    }

    let mut updated = String::with_capacity(original.len());
    updated.push_str(&original[..start_end]);
    updated.push_str(&new_region);
    updated.push_str(&original[end..]);
    fs::write(path, &updated).map_err(|err| format!("writing {}: {err}", path.display()))?;
    Ok(true)
}

/// Collapses all whitespace runs to single spaces, so text that differs only in line-wrapping
/// compares equal.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_doc_comment() {
        let toml = "
[features]
default = [\"std\"]
# Enables std support.
std = []
# Optional extra.
extra = []
";
        let features = parse_features(toml).unwrap();
        assert_eq!(features.len(), 2);
        assert_eq!(features[0].name, "extra");
        assert!(!features[0].is_default);
        assert_eq!(features[0].doc, vec!["Optional extra."]);
        assert_eq!(features[1].name, "std");
        assert!(features[1].is_default);
        assert_eq!(features[1].doc, vec!["Enables std support."]);
    }

    #[test]
    fn double_underscore_prefixed_features_are_skipped() {
        let toml = "
[features]
default = [\"std\"]
# Enables std support.
std = []
# Internal, not for consumers.
__math = []
";
        let features = parse_features(toml).unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].name, "std");
    }

    #[test]
    fn multiline_doc_comment_is_contiguous_only() {
        let toml = "
[features]
# unrelated blank-separated comment

# First line of doc.
# Second line of doc.
thing = []
";
        let features = parse_features(toml).unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(
            features[0].doc,
            vec!["First line of doc.", "Second line of doc."]
        );
    }

    #[test]
    fn strips_doc_links_for_readme() {
        assert_eq!(
            strip_doc_links("See [`Color::to_indexed`](color::Color::to_indexed) and [`Color`]."),
            "See `Color::to_indexed` and `Color`."
        );
    }

    #[test]
    fn renders_default_and_optional_markers() {
        let features = vec![
            Feature {
                name: "a".into(),
                is_default: true,
                doc: vec!["Does a.".into()],
            },
            Feature {
                name: "b".into(),
                is_default: false,
                doc: vec!["Does b.".into()],
            },
        ];
        let rustdoc = render(&features, Flavor::Rustdoc);
        assert!(rustdoc.contains("Default features: `a`."));
        assert!(rustdoc.contains("### `a`"));
        assert!(rustdoc.contains("🟢 Enabled by default."));
        assert!(rustdoc.contains("### `b`"));
        assert!(rustdoc.contains("⚪ Optional."));
        assert!(!rustdoc.contains("<details>"));
    }

    #[test]
    fn no_defaults_renders_fallback_intro() {
        let features = vec![Feature {
            name: "a".into(),
            is_default: false,
            doc: vec!["Does a.".into()],
        }];
        let rendered = render(&features, Flavor::Rustdoc);
        assert!(rendered.starts_with("This crate has no default features"));
        assert!(!rendered.contains("<details>"));
    }

    #[test]
    fn readme_wraps_body_in_details_with_blank_lines() {
        let features = vec![Feature {
            name: "a".into(),
            is_default: true,
            doc: vec!["Does a.".into()],
        }];
        let readme = render(&features, Flavor::Readme);
        assert!(readme.starts_with("<details>\n\n<summary>Default features: `a`.</summary>\n\n"));
        assert!(readme.contains("</summary>\n\n### `a`"));
        assert!(readme.trim_end().ends_with("</details>"));
        assert!(readme.contains("Does a.\n\n</details>"));
    }

    #[test]
    fn readme_no_defaults_uses_short_summary_and_keeps_long_intro_in_body() {
        let features = vec![Feature {
            name: "a".into(),
            is_default: false,
            doc: vec!["Does a.".into()],
        }];
        let readme = render(&features, Flavor::Readme);
        assert!(readme.starts_with(
            "<details>\n\n<summary>Features: all optional, none enabled by default.</summary>\n\n"
        ));
        assert!(readme.contains("This crate has no default features"));
    }

    #[test]
    fn update_markers_tolerates_prettier_style_rewrapping() {
        // Same words, different line breaks (as `prettier`'s `proseWrap` would produce) must not
        // count as drift, or this tool and `prettier` would fight over the same lines forever.
        let dir = env::temp_dir().join(format!("gen-features-test-rewrap-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("README.md");
        fs::write(
            &path,
            "<!-- gen-features:start -->\none two three\nfour five\n<!-- gen-features:end -->\n",
        )
        .unwrap();

        let changed = update_markers(&path, "one two\nthree four five", "", true).unwrap();
        assert!(
            !changed,
            "identical words with different wrapping must not be reported as stale"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn update_markers_replaces_body_and_is_idempotent() {
        let dir = env::temp_dir().join(format!("gen-features-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lib.rs");
        fs::write(
            &path,
            "//! # Features\n//!\n//! <!-- gen-features:start -->\n//! old\n//! <!-- gen-features:end -->\n//!\n//! # Rest\n",
        )
        .unwrap();

        let changed = update_markers(&path, "new\n\n### x", "//! ", false).unwrap();
        assert!(changed);
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("//! new"));
        assert!(!updated.contains("//! old"));
        assert!(updated.contains("//! # Rest"));

        let changed_again = update_markers(&path, "new\n\n### x", "//! ", false).unwrap();
        assert!(!changed_again);

        fs::remove_dir_all(&dir).unwrap();
    }
}
