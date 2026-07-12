# AGENTS.md (crates/widgets)

## Widget structs must actually be builders

Every type in `src/widget/` (one file per widget: `Panel`, `Gauge`, `ProgressBar`, `Scrollbar`, ...)
is a builder, not a struct-shaped function call. If `new()` takes every field and there isn't at
least one chainable setter, it isn't a builder -- it's a function wearing a struct as a costume, and
it should have stayed a free function in `draw/` instead (see `fill_rect`, which correctly stayed
one).

**The rule:** `new()` takes only the arguments the widget cannot mean anything without -- the actual
content (a value, a label, a slice of samples/rows/messages). Every other knob -- style, offset,
title, threshold -- gets a sensible default (usually `Style::new()` or `0`) baked into `new()`, and
is set afterwards through a chainable `#[must_use] fn field(mut self, ...) -> Self` method.
`Panel::title` and `Log::offset` are the reference examples; copy their shape.

```rust
// good: new() takes only the content; style is optional with a default,
// set via a chainable method.
impl ProgressBar {
    #[must_use]
    pub fn new(value: u32, max: u32) -> Self {
        Self { value, max, filled_style: Style::new(), empty_style: Style::new() }
    }

    #[must_use]
    pub const fn filled_style(mut self, style: Style) -> Self {
        self.filled_style = style;
        self
    }
}
```

```rust
// bad: everything crammed into new(), nothing chainable. This is a
// function call that happens to be spelled as a struct literal.
impl ProgressBar {
    pub const fn new(value: u32, max: u32, filled_style: Style, empty_style: Style) -> Self { ... }
}
```

This was gotten wrong once already: the first pass at `ProgressBar`, `BoxBorder`, `Modal`, and
`Scrollbar` took every field in `new()` with zero builder methods, defeating the entire point of
migrating away from free-function call sites. Don't repeat it.

### When it's fine to skip the builder

If every field genuinely has no reasonable default -- `Table`'s `headers`/ `widths`/`rows`,
`Gauge`'s `label`/`ratio`, `Sparkline`'s `samples` -- don't force a builder split just for form. The
test is honest: could you write `Style::new()` or `0` as a plausible default for this field without
lying about what the widget draws? If yes, it's a builder knob. If no (the widget is meaningless
without it), it belongs in `new()`.

### Losing `const fn`

A `new()` that defaults a `Style` field via `Style::new()` can't be `const fn`
(`Style::new`/`Style::default` aren't `const`). That's an acceptable trade for a real builder --
these are immediate-mode widgets rebuilt every frame, not compile-time constants. Chainable setter
methods that only assign a field (no function calls) stay `const fn` regardless.

## Content parameters: `&str` is fine, but check what you lose

`Gauge`/`StatBar` take `label: &str`, not `label: Text` -- that's correct, not a corner. This is
0.1.0-alpha: swapping `&str` for `impl Into<Text<'a>>` later is a free, non-breaking change (a
blanket `From<&str> for Text` keeps every existing call site compiling), so there's no irreversible
commitment being made by keeping it a `&str` today.

But "the type is fine" doesn't mean "nothing to check" -- before leaving a content parameter as a
bare `&str`, verify the caller isn't losing configuration they'd reasonably want. `Gauge`/`StatBar`
originally hardcoded their label's color with no way to override it at all, which was the real gap
(not the type). The fix was a `label_style: Style` field defaulting to the original hardcoded color,
set via a chainable `label_style(self, Style) -> Self` -- the same builder-knob pattern as
everything else in this crate, not a type change. Reach for `impl Into<Text<'a>>` only once a widget
genuinely needs richer-than-one-style content (multiple spans, mixed formatting within the label); a
single overridable `Style` is enough for everything here today.

## `draw/` vs `widget/`

`draw/` holds only things that are genuinely just functions: `fill_rect` (a one-shot fill with no
configuration worth building) and `thumb_geometry`/`offset_for_pos` (pure position/size math with no
`Terminal` involved, reused for hit-testing independently of drawing a `Scrollbar`). Everything else
is a widget in `widget/`, one file each, and owns its own drawing logic directly -- widgets do not
wrap or delegate to a same-shaped free function in `draw/` (that was the old, since-removed
`widget/*.rs` "thin adapter over `draw::*`" design; it meant two ways to do the same thing and got
explicitly rejected). Widgets _may_ compose other _widgets_ (e.g. `Panel` composes `BoxBorder`,
`Modal` composes `Panel`, `Log` composes `PrintLine`) -- that's the intended way to share drawing
logic, not a private free-function core underneath both.

## See also

- Root `AGENTS.md` for workspace-wide rules (correctness gate, testing, commit conventions).
- `STYLE_GUIDE.md` for general Rust API conventions not specific to this crate.
