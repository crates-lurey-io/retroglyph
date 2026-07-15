#!/usr/bin/env bash
# Syncs repo labels to match .github/labels.yml: creates missing labels, updates color/description
# on existing ones, and reports (without deleting) any labels present on GitHub but absent from the
# file, so removals stay an explicit human decision. Requires `gh` and `yq`.
#
# Usage: .github/scripts/sync-labels.sh [--delete-extra]

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

LABELS_FILE=".github/labels.yml"
DELETE_EXTRA=false
[[ "${1:-}" == "--delete-extra" ]] && DELETE_EXTRA=true

command -v yq >/dev/null || { echo "yq is required (brew install yq)" >&2; exit 1; }
command -v gh >/dev/null || { echo "gh is required" >&2; exit 1; }

existing=$(gh label list --limit 200 --json name --jq '.[].name')
wanted=$(yq -r '.[].name' "$LABELS_FILE")

count=$(yq '. | length' "$LABELS_FILE")
for i in $(seq 0 $((count - 1))); do
  name=$(yq -r ".[$i].name" "$LABELS_FILE")
  color=$(yq -r ".[$i].color" "$LABELS_FILE")
  desc=$(yq -r ".[$i].description" "$LABELS_FILE")

  if grep -qxF "$name" <<<"$existing"; then
    echo "update: $name"
    gh label edit "$name" --color "$color" --description "$desc" >/dev/null
  else
    echo "create: $name"
    gh label create "$name" --color "$color" --description "$desc" >/dev/null
  fi
done

extra=$(comm -23 <(sort <<<"$existing") <(sort <<<"$wanted"))
if [[ -n "$extra" ]]; then
  echo
  echo "Labels on GitHub but not in $LABELS_FILE:"
  echo "$extra" | sed 's/^/  /'
  if $DELETE_EXTRA; then
    while IFS= read -r name; do
      echo "delete: $name"
      gh label delete "$name" --yes
    done <<<"$extra"
  else
    echo "(re-run with --delete-extra to remove them)"
  fi
fi
