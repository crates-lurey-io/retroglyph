#!/usr/bin/env bash
# Generates target/doc/crates/index.html (the API Documentation landing page linked from the
# docs site's root) by listing every publishable workspace crate with a lib target -- the same
# crate set tools/gen-llms-txt.sh writes llms.txt/llms-full.txt for, and the same crate set
# `cargo doc` produces a `target/doc/<lib_name>/` rustdoc directory for.
#
# Previously this table was hand-maintained directly in the HTML, which silently went stale as
# crates were added (retroglyph-gl, retroglyph-terminal, retroglyph-terminal-wasm were all
# missing from it). Deriving the row list from `cargo metadata` instead means a new crate shows
# up here automatically the next time docs are built, with nothing to remember to edit.
#
# Usage: tools/gen-crates-index.sh [out-dir]
#   out-dir defaults to target/doc, matching gen-llms-txt.sh and `cargo doc`'s own output root.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

out_dir="${1:-target/doc}"
crates_out="$out_dir/crates"
mkdir -p "$crates_out"

rows=""
while IFS=$'\t' read -r name lib_name; do
  [ -n "$lib_name" ] || continue # skip crates with no library target (e.g. cargo-bin)
  rows="$rows<tr>\n"
  rows="$rows            <td class=\"crate\">$name</td>\n"
  rows="$rows            <td><a href=\"../$lib_name/index.html\">rustdoc</a></td>\n"
  rows="$rows            <td><a href=\"../$lib_name/llms-full.txt\">llms-full.txt</a></td>\n"
  rows="$rows            <td><a href=\"../$lib_name/llms.txt\">llms.txt</a></td>\n"
  rows="$rows          </tr>\n"
done < <(
  cargo metadata --no-deps --format-version=1 |
    jq -r '.packages[] | select(.publish != []) | [.name, (.targets[] | select(.kind[] == "lib") | .name)] | @tsv' |
    sort
)

# See tools/build-wasm-examples.sh's comment on __ROWS__ for why this is bash parameter
# expansion rather than sed: the accumulated rows are joined by literal `\n`s that need
# `printf '%b'` expansion, and once there's more than a handful of rows that breaks sed's
# `s///` replacement-side newline handling.
template="$(cat "$root_dir/docs/templates/crates/index-template.html")"
rows_expanded="$(printf '%b' "$rows")"
echo "${template//__ROWS__/$rows_expanded}" >"$crates_out/index.html"

echo "Wrote $crates_out/index.html"
