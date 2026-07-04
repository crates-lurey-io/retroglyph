#!/usr/bin/env bash
# Guards docs.yml's three WASM packaging steps (Software/Headless/Terminal)
# against packaging a stale .wasm left over from a partial CI failure.
#
# Software, Headless, and Terminal all build the *same* example name to the
# *same* output path (target/wasm32-unknown-unknown/release/examples/<name>.wasm
# -- Cargo feature flags don't affect the artifact filename). Each variant is
# built and packaged (wasm-bindgen'd into target/doc/) before the next
# variant's build runs, so a successful build isn't clobbered by the next
# stage before it's ever wasm-bindgen'd.
#
# But every build step is `continue-on-error: true`, so a *later* build can
# fail after an *earlier* one succeeded, leaving the earlier variant's .wasm
# sitting at the shared path when the later variant's package step runs.
# Packaging that stale .wasm would produce a broken demo (wrong or missing
# #[wasm_bindgen] exports for the template it's paired with) that's still
# linked from the index.
#
# Usage: verify-wasm-exports.sh <js-file> <variant>
#
# <variant> is one of: software, headless, terminal. Exits 0 (silently) if
# the .js file has every export that variant's template needs, and (for
# "software" specifically) none of the other variants' exports -- a stale
# Headless/Terminal .wasm would otherwise slip past a check that only looks
# for required symbols. Exits 1 with a message on stderr otherwise, so the
# caller can skip packaging that example instead of shipping a broken demo.
set -euo pipefail

js_file="$1"
variant="$2"

if [ ! -f "$js_file" ]; then
  echo "verify-wasm-exports: $js_file does not exist" >&2
  exit 1
fi

headless_exports=(wasm_headless_init wasm_headless_push_key wasm_headless_tick)
terminal_exports=(wasm_terminal_example_init wasm_terminal_example_resize wasm_terminal_example_push_key wasm_terminal_example_tick)

has_export() {
  grep -qF "$1" "$js_file"
}

require_exports() {
  local name
  for name in "$@"; do
    if ! has_export "$name"; then
      echo "verify-wasm-exports: $js_file missing expected '$name' export for variant '$variant' -- likely a stale .wasm from a different, earlier-built variant" >&2
      return 1
    fi
  done
  return 0
}

reject_exports() {
  local name
  for name in "$@"; do
    if has_export "$name"; then
      echo "verify-wasm-exports: $js_file unexpectedly has '$name' export for variant '$variant' -- likely a stale .wasm from a different, later-built variant" >&2
      return 1
    fi
  done
  return 0
}

case "$variant" in
  headless)
    require_exports "${headless_exports[@]}"
    ;;
  terminal)
    require_exports "${terminal_exports[@]}"
    ;;
  software)
    # Software's entry point is `wasm_main` (a `#[wasm_bindgen(start)]`
    # function called automatically on module init), not a set of named
    # exports the JS template calls explicitly the way Headless/Terminal are
    # -- so the only thing to check here is the *absence* of the other two
    # variants' exports, which would indicate this .wasm is actually a
    # leftover Headless or Terminal build.
    reject_exports "${headless_exports[@]}" "${terminal_exports[@]}"
    ;;
  *)
    echo "verify-wasm-exports: unknown variant '$variant' (expected software, headless, or terminal)" >&2
    exit 1
    ;;
esac
