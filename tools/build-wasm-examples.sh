#!/usr/bin/env bash
# Builds every example in examples/examples/*.rs for each WASM-capable
# backend variant via tools/build-wasm-example.sh, and writes a top-level
# index.html linking to all of them.
#
# Convention over configuration (see docs/design/018-terminal-family-split.md
# and the examples crate's own doc comments): every example is assumed to
# support every variant, so there is no manifest to keep in sync -- the
# example list comes from `ls examples/examples/*.rs`, and this script just
# builds all three variants for each one. If an example genuinely can't
# support a variant (rare; none do today), that's a build failure this
# script surfaces, not a silent skip.
#
# Usage: tools/build-wasm-examples.sh [output-dir]
#   output-dir defaults to target/doc/examples

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples_dir="$repo_root/examples"
out_dir="${1:-$repo_root/target/doc/examples}"
templates_dir="$repo_root/docs/templates/examples"

variants_label=(Headless Terminal Software)
variants_dir=(headless terminal software)

mkdir -p "$out_dir"
rows=""

for example_path in "$examples_dir"/examples/*.rs; do
  example="$(basename "$example_path" .rs)"
  echo "== $example =="
  cells=""

  for i in "${!variants_dir[@]}"; do
    variant="${variants_dir[$i]}"
    label="${variants_label[$i]}"

    echo "-- $example / $label --"
    "$repo_root/tools/build-wasm-example.sh" "$example" "$variant" "$out_dir/$example/$variant"

    cells="$cells<td><a href=\"./$example/$variant/\">Run</a></td>"
  done

  rows="$rows<tr><td class=\"example\">$example</td>$cells</tr>\n"
done

sed "s#__ROWS__#$(printf '%b' "$rows")#" "$templates_dir/index-template.html" > "$out_dir/index.html"

echo "Wrote $out_dir/index.html and one directory per example/variant."
