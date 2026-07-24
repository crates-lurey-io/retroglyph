#!/usr/bin/env python3
"""Splits the workspace-wide nextest JUnit report into one file per Codecov flag.

`cargo nextest run --workspace` (see `just test-ci`) writes a single `junit.xml` with one
`<testsuite>` per test binary -- one per library crate (`retroglyph-core`, `retroglyph-widgets`,
...) plus one per `examples/examples/*.rs` example (`retroglyph-examples::01_hello_world`, ...).

Codecov Test Analytics' `flags` filter is populated per *upload*, not per `<testsuite>` inside a
single upload (see the CI comment on the Upload test results step in .github/workflows/ci.yml for
why one combined upload can't carry multiple flags without lying about which tests belong to
which crate -- Codecov's own Flags docs warn about exactly this). So getting an accurate `core`,
`terminal`, etc. flag on the Tests dashboard means uploading each crate's `<testsuite>`s
separately, tagged with that crate's flag. Re-running nextest per crate would work too, but is
pure overhead: the workspace run already produced everything in one JUnit file, correctly grouped
by binary. This script just re-partitions that existing file instead of re-running tests.

Usage: split-junit-flags.py <workspace-junit.xml> <output-dir>

Writes `<output-dir>/<flag>.xml` for every flag in codecov.yml's `flags:` section that has at
least one matching `<testsuite>`, plus `<output-dir>/unflagged.xml` for anything left over (today,
just the examples crate's snapshot tests -- they exercise multiple backend crates at once, so no
single crate flag fits them).
"""

from __future__ import annotations

import sys
import xml.etree.ElementTree as ET
from pathlib import Path

# Keep in sync with the `flags:` section of codecov.yml (one flag per crates/* directory).
FLAGS = [
    "core",
    "terminal",
    "crossterm",
    "terminal-wasm",
    "software",
    "gl",
    "window",
    "widgets",
]


def flag_for_suite(suite_name: str) -> str | None:
    """Maps a nextest `<testsuite name="...">` (binary id) to its Codecov flag, if any.

    Binary ids look like `retroglyph-core` (the crate's own lib/unit tests) or
    `retroglyph-crossterm::non_tty` (a secondary `[[test]]` binary in that same crate) --
    strip the `retroglyph-` prefix and any `::binary` suffix, then match against FLAGS.
    """
    if not suite_name.startswith("retroglyph-"):
        return None
    crate = suite_name.removeprefix("retroglyph-").split("::", 1)[0]
    return crate if crate in FLAGS else None


def main() -> None:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <workspace-junit.xml> <output-dir>", file=sys.stderr)
        raise SystemExit(2)

    src_path, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    root = ET.parse(src_path).getroot()
    if root.tag != "testsuites":
        raise SystemExit(f"expected <testsuites> root, got <{root.tag}>")

    buckets: dict[str, list[ET.Element]] = {}
    for suite in root.findall("testsuite"):
        name = suite.get("name", "")
        flag = flag_for_suite(name) or "unflagged"
        buckets.setdefault(flag, []).append(suite)

    out_dir.mkdir(parents=True, exist_ok=True)
    for flag, suites in buckets.items():
        out_root = ET.Element("testsuites", {"name": root.get("name", "nextest-run")})
        for attr in ("tests", "failures", "errors", "skipped"):
            total = sum(int(s.get(attr, 0)) for s in suites)
            out_root.set(attr, str(total))
        out_root.extend(suites)
        out_path = out_dir / f"{flag}.xml"
        ET.ElementTree(out_root).write(out_path, encoding="unicode", xml_declaration=True)
        print(f"{out_path}: {len(suites)} suite(s), {out_root.get('tests')} test(s)")


if __name__ == "__main__":
    main()
