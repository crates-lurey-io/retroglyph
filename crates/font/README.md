# retroglyph-font

Bitmap glyph fonts and CP437 mapping for
[retroglyph](https://github.com/crates-lurey-io/retroglyph)'s graphical backends.

`no_std`, dependency-free. Provides [`BitmapFont`] (a static 1-bit-per-pixel glyph table),
[`FallbackFontChain`] for layering fonts, and Unicode->CP437 glyph resolution. This is the shared
glyph-source layer both `retroglyph-software` (CPU rasterizer) and `retroglyph-gl` (GPU glyph atlas)
build on, so their text output stays pixel-identical.

## Features

| Feature        | Effect                                                                  |
| -------------- | ----------------------------------------------------------------------- |
| `default-font` | Embeds the Unscii 16 font (`unscii16::FONT`), 256 CP437 glyphs at 8x16. |

Off by default: supply your own font via `BitmapFont::new` and pay nothing for the embedded atlas.

## License

Same as the workspace. The embedded Unscii 16 data is derived from the public-domain/CC0
`unscii-16.hex` (<https://github.com/viznut/unscii>); see the `unscii16` module docs for the four
CP437 codepoints filled in with original pixel art.
