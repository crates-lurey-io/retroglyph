#!/usr/bin/env bash
# Benchmarks the CURRENT working copy (whatever's on disk right now, committed or not) against one
# other git commit, and prints a comparison table.
#
# The current working copy is never touched: it's benchmarked in place. The comparison ref gets
# its own throwaway `git worktree` so checking it out can't disturb your actual working directory
# (dirty changes included).
#
# How the comparison itself works:
#   - Both runs are pointed at the *same* `CARGO_TARGET_DIR` (this repo's `target/`), because
#     criterion's `--save-baseline <name>` writes to `target/criterion/<bench>/<name>/` keyed only
#     by that name, not by source checkout. Sharing the target dir is what lets a baseline saved
#     from the throwaway worktree be compared against a run from the real working directory.
#   - `cargo bin critcmp` (see [workspace.metadata.bin] in root Cargo.toml) then diffs the two
#     named baselines into a table.
#
# Usage: tools/bench-compare.sh [-b <bench-name>] [<ref>]
#   <ref>            git commit-ish to compare against (default: origin/main). Must already
#                    contain benches/ (an older ref predating this benchmark crate won't work).
#   -b <bench-name>  criterion bench target under benches/benches/ to run (default: grid_diff)
#
# Extra args after `--` are forwarded to the criterion run, e.g. to filter to one benchmark or
# shorten sample time for a quick check:
#   tools/bench-compare.sh -- grid_diff/80x24 --sample-size 20
#
# Examples:
#   tools/bench-compare.sh                    # origin/main vs. current working copy
#   tools/bench-compare.sh HEAD~5              # 5 commits back vs. current working copy
#   tools/bench-compare.sh -b grid_diff v0.3.0
#
# Requires: git (a worktree-capable checkout).

set -euo pipefail

# Parsed by hand rather than `getopts`: getopts treats a bare `--` as its own end-of-options
# marker and silently swallows it while consuming `-b`, so by the time we'd get to check for a
# `--` separator ourselves (to know where pass-through criterion args start) it would already be
# gone -- the first pass-through arg would get mistaken for the <ref> positional instead.
bench_name="grid_diff"
baseline_ref="origin/main"
ref_set=0
extra_args=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    -b)
      bench_name="${2:?"-b requires a bench name"}"
      shift 2
      ;;
    -h | --help)
      sed -n '2,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    --)
      shift
      extra_args=("$@")
      break
      ;;
    -*)
      echo "usage: $0 [-b <bench-name>] [<ref>] [-- <criterion-args>]" >&2
      exit 1
      ;;
    *)
      if [ "$ref_set" -eq 1 ]; then
        echo "usage: $0 [-b <bench-name>] [<ref>] [-- <criterion-args>]" >&2
        exit 1
      fi
      baseline_ref="$1"
      ref_set=1
      shift
      ;;
  esac
done

repo_root="$(git rev-parse --show-toplevel)"
cargo_target_dir="$repo_root/target"

worktree_dir="$(mktemp -d "/tmp/rg-bench-baseline.XXXXXX")"
cleanup() {
  git -C "$repo_root" worktree remove --force "$worktree_dir" >/dev/null 2>&1 || rm -rf "$worktree_dir"
  git -C "$repo_root" worktree prune >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> Checking out '$baseline_ref' into throwaway worktree $worktree_dir" >&2
git -C "$repo_root" worktree add --detach "$worktree_dir" "$baseline_ref" >&2

run_bench() {
  local dir="$1" save_name="$2"
  echo "==> Benchmarking in $dir (saving baseline '$save_name')" >&2
  (
    cd "$dir"
    CARGO_TARGET_DIR="$cargo_target_dir" cargo bench -p retroglyph-benches --bench "$bench_name" -- \
      --save-baseline "$save_name" "${extra_args[@]}"
  )
}

run_bench "$worktree_dir" baseline
run_bench "$repo_root" current

echo
echo "==> $baseline_ref (baseline) vs. current working copy:"
cargo bin critcmp baseline current
