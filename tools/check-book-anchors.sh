#!/usr/bin/env bash
# Verifies every `{{#include <path>:<anchor>}}` under docs/src/ points at a file that exists and,
# when an anchor is given, at an `ANCHOR: <name>` marker that still exists in that file.
#
# `mdbook build` (0.5.4) does not fail its own process on either problem: a missing *file* logs an
# `ERROR` line to stderr but still exits 0, and a missing *anchor* in an otherwise valid file logs
# nothing at all and silently renders an empty snippet -- confirmed by hand against this version.
# That defeats the whole point of retroglyph#487 (renamed/removed source code should break the
# book's build, not quietly leave stale-looking prose behind), so `just book` runs this script
# itself instead of trusting mdbook's own exit code.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src_dir="$root_dir/docs/src"

status=0

while IFS= read -r -d '' md_file; do
  # Matches `{{#include path}}` and `{{#include path:anchor}}` (also mdbook's `:N:M`/`:N`
  # line-range forms, which this script doesn't validate -- only the anchor form needs a
  # same-file marker to check).
  while IFS= read -r directive; do
    target="$(sed -E 's/\{\{#include[[:space:]]+([^:}[:space:]]+).*/\1/' <<< "$directive")"
    # Not canonicalized (`realpath -m` isn't portable: BSD realpath, macOS's default, has no
    # missing-path mode) -- `..` segments resolve fine for a plain existence/grep check without
    # it, so `resolved` stays a plain, possibly `..`-containing path.
    resolved="$(dirname "$md_file")/$target"

    if [ ! -f "$resolved" ]; then
      echo "check-book-anchors: ${md_file#"$root_dir"/}: include target not found: $target" >&2
      status=1
      continue
    fi

    anchor="$(sed -E 's/\{\{#include[^:}]+:([A-Za-z0-9_-]+)\}\}/\1/' <<< "$directive")"
    if [ "$anchor" != "$directive" ] && ! grep -qE "ANCHOR:[[:space:]]*${anchor}([[:space:]]|$)" "$resolved"; then
      echo "check-book-anchors: ${md_file#"$root_dir"/}: anchor '$anchor' not found in ${resolved#"$root_dir"/}" >&2
      status=1
    fi
  done < <(grep -oE '\{\{#include[^}]+\}\}' "$md_file" || true)
done < <(find "$src_dir" -name '*.md' -print0)

if [ "$status" -ne 0 ]; then
  echo "check-book-anchors: one or more {{#include}} directives are broken; see errors above." >&2
fi

exit "$status"
