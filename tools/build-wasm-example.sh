#!/usr/bin/env bash
# Builds one example for one WASM-capable backend variant, wasm-bindgen's the
# result, and packages it with the matching HTML template from
# docs/templates/examples/ into a destination directory. Shared by
# tools/build-wasm-examples.sh (loops over every example/variant for the docs
# site) and the CLI runner's local WASM preview (one example/variant at a
# time, served on demand) -- both need byte-for-byte the same packaging, so
# it lives in exactly one place.
#
# Requires: the wasm32-unknown-unknown target, and `cargo bin` (cargo-run-bin)
# to resolve `wasm-bindgen` at the version pinned in the workspace root
# Cargo.toml's [workspace.metadata.bin]. Run `just setup-wasm` first if
# `cargo bin wasm-bindgen --version` hasn't been built yet.
#
# Usage: tools/build-wasm-example.sh <example> <headless|terminal|software> <dest-dir>

set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <example> <headless|terminal|software> <dest-dir>" >&2
  exit 1
fi

example="$1"
variant="$2"
dest="$3"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples_dir="$repo_root/examples"
templates_dir="$repo_root/docs/templates/examples"

case "$variant" in
  headless) features=wasm-headless; template=headless-template.html ;;
  terminal) features=wasm-terminal; template=terminal-template.html ;;
  software) features=software; template=software-template.html ;;
  *)
    echo "unknown variant: $variant (expected headless, terminal, or software)" >&2
    exit 1
    ;;
esac

cargo build \
  --manifest-path "$examples_dir/Cargo.toml" \
  --target wasm32-unknown-unknown \
  --release \
  --example "$example" \
  --features "$features"

mkdir -p "$dest"
(
  cd "$repo_root"
  cargo bin wasm-bindgen \
    --target web \
    --out-dir "$dest" \
    --out-name "$example" \
    "target/wasm32-unknown-unknown/release/examples/$example.wasm"
)

sed "s/__EXAMPLE__/$example/g" "$templates_dir/$template" > "$dest/index.html"
if [ "$variant" = software ]; then
  sed "s/__EXAMPLE__/$example/g" \
    "$templates_dir/software-template.manifest.webmanifest" \
    > "$dest/manifest.webmanifest"
fi
