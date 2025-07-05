# retroglyph

2D pseudographic terminal.

[![Test](https://github.com/crates-lurey-io/retroglyph/actions/workflows/test.yml/badge.svg)](https://github.com/crates-lurey-io/retroglyph/actions/workflows/test.yml)
[![Crates.io Version](https://img.shields.io/crates/v/retroglyph)](https://crates.io/crates/retroglyph)
[![codecov](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=Z3VUWA3WYY)](https://codecov.io/gh/crates-lurey-io/retroglyph)

Retroglyph is a code-page 437 terminal renderer and lightweight UI framework,
similar to [BearLibTerminal](http://foo.wyrd.name/en:bearlibterminal).

This project is _under heavy development_ and is not yet ready for use.

## Features

Currently `retroglyph` has no required dependencies, and supports `no_std`[^1].

[^1]: I have delusions of using this on a microcontroller.

### `backend-software`

Enables the software backend.

Uses [`softbuffer`][] to render a terminal as a `&mut [u32]` pixel buffer.

[`softbuffer`]: https://crates.io/crates/softbuffer

## Comparison

The following is a comparison of features with [BearLibTerminal](http://foo.wyrd.name/en:bearlibterminal:reference):

Initialization and configuration:
- [ ] `open`
- [ ] `close`
- [ ] `set`

Output state:
- [ ] `color`
- [ ] `bkcolor`
- [ ] `composition`
- [ ] `layer`

Output:
- [ ] `clear`
- [ ] `clear_area`
- [ ] `crop`
- [ ] `refresh`
- [ ] `put`
- [ ] `pick`
- [ ] `pick_color`
- [ ] `pick_bkcolor`
- [ ] `put_ext`
- [ ] `print`
- [ ] `measure`

Input:
- [ ] `state`
- [ ] `check`
- [ ] `has_input`
- [ ] `read`
- [ ] `peek`
- [ ] `read_str`

Utility:
- [ ] `delay`
- [ ] `color_from_name`
- [ ] `color_from_argb`

## Contributing

This project uses [`just`][] to run commands the same way as the CI:

- `cargo just check` to check formatting and lints.
- `cargo just coverage` to generate and preview code coverage.
- `cargo just doc` to generate and preview docs.
- `cargo just test` to run tests.

[`just`]: https://crates.io/crates/just

For a full list of commands, see the [`Justfile`](./Justfile).
