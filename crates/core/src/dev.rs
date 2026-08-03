//! Build-mode vocabulary: which diagnostics a build compiles in.
//!
//! retroglyph emits diagnostics that exist purely to shorten the debugging loop: a warning that
//! a sprite is bigger than the cells reserved for it, a warning that a tint was set on a cell
//! that resolved to a font glyph rather than a sprite. Each one costs something to produce (a
//! formatted message, and usually a side table so a 60fps redraw loop reports each offender once
//! instead of every frame), and none of it is worth anything in a shipped game, where nobody is
//! reading the log.
//!
//! [`BuildMode::CURRENT`] names which kind of build this is, and [`dev_only!`] gates a block on
//! it. In a release build the const is `false`, the branch folds away, and everything inside it
//! (message strings, the bookkeeping that dedupes them) is dropped as dead code.
//!
//! ```
//! use retroglyph_core::dev_only;
//!
//! # fn report(_: &str, _: usize) {}
//! # let cache_misses = 3;
//! dev_only!({
//!     if cache_misses > 0 {
//!         // Costs nothing in a release build: neither the check nor the message survives.
//!         report("glyphs missed the sprite cache", cache_misses);
//!     }
//! });
//! ```
//!
//! # Two modes, not three
//!
//! Engines that own their whole toolchain usually expose three build modes. Flutter's
//! `debug`/`profile`/`release` is the clearest version: `debug` is unoptimized with every
//! assertion live, `profile` is optimized but keeps enough instrumentation to attribute a frame
//! budget, and `release` is what ships.
//!
//! Cargo has no `profile` mode in that sense. A profiling build is a release build that keeps
//! debug symbols (`[profile.profiling] inherits = "release"`, plus `debug = true`), and it is
//! *supposed* to be one: measuring a build whose diagnostics differ from the shipped build
//! measures the wrong program. So there are two modes here, and a profiling build resolves to
//! [`Release`](BuildMode::Release).
//!
//! That is also why the gate is written as "is this a dev build" rather than "is this not a
//! release build". Flutter's own guidance on its `kReleaseMode` constant is to prefer `kDebugMode`
//! or `assert` precisely because gating on *not release* is what makes a profile build behave
//! unlike the release build it is meant to predict.
//!
//! # How a mode is chosen
//!
//! | Build | [`BuildMode::CURRENT`] |
//! | --- | --- |
//! | `cargo build`, `cargo test`, `cargo run` | [`Dev`](BuildMode::Dev) |
//! | `cargo build --release` | [`Release`](BuildMode::Release) |
//! | a profiling profile inheriting `release` | [`Release`](BuildMode::Release) |
//! | any build with the `dev` feature on | [`Dev`](BuildMode::Dev) |
//! | any build with `-C debug-assertions=on` | [`Dev`](BuildMode::Dev) |
//!
//! The default signal is `debug_assertions`, which Cargo turns on for the `dev` profile and off
//! for `release`. It follows whichever profile the consumer built with, so a game gets
//! diagnostics from `cargo run` and none from `cargo run --release` without configuring anything.
//!
//! The `dev` feature forces [`Dev`](BuildMode::Dev) on regardless, for an optimized build that
//! still reports. This is the equivalent of Unity's "Development Build" checkbox or Bevy's `dev`
//! feature: release codegen, because an unoptimized build of a renderer is too slow to reproduce
//! anything frame-dependent, but with the instrumentation left in.
//!
//! # Turning diagnostics off in a dev build
//!
//! There is no feature for this. Cargo features are additive, so a `no-dev` feature
//! would be silently defeated by any other crate in the graph that wanted diagnostics.
//!
//! Every diagnostic in this workspace goes through the `log` crate, so the two working controls
//! are the consumer's own log filter at runtime, and `log`'s `max_level_*` /
//! `release_max_level_*` features, which drop the calls at compile time. Those cut deeper than
//! this module does: they apply to every `log` user in the graph, not just retroglyph.

/// Which diagnostics this build compiles in.
///
/// Read [`CURRENT`](Self::CURRENT) for this build's mode, or use [`dev_only!`](crate::dev_only)
/// to gate a block on it. See the [module docs](self) for how a mode is chosen and why there are
/// two of them rather than three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BuildMode {
    /// Development diagnostics are compiled in.
    ///
    /// Selected by `debug_assertions` (so: any `cargo` command that has not been pointed at the
    /// `release` profile) or by this crate's `dev` feature.
    Dev,
    /// Development diagnostics are compiled out.
    ///
    /// Selected by the `release` profile, and by any profile inheriting it, which includes a
    /// profiling build. Enable the `dev` feature to get an optimized build that still reports.
    Release,
}

impl BuildMode {
    /// The mode this build was compiled in.
    ///
    /// A `const`, so a branch on it folds away and the untaken side is dropped as dead code.
    pub const CURRENT: Self = if cfg!(debug_assertions) || cfg!(feature = "dev") {
        Self::Dev
    } else {
        Self::Release
    };

    /// Whether this is [`Dev`](Self::Dev).
    #[must_use]
    pub const fn is_dev(self) -> bool {
        matches!(self, Self::Dev)
    }
}

/// Whether this build compiles in development diagnostics: [`BuildMode::CURRENT`] as a `bool`.
///
/// Prefer [`dev_only!`](crate::dev_only) for gating a block. Reach for this constant directly
/// when the shape of the code makes a macro awkward, such as an early return or a struct field
/// that only one mode populates.
pub const DEV: bool = BuildMode::CURRENT.is_dev();

/// Runs `body` only in a build that compiles in development diagnostics.
///
/// Expands to `if DEV { body }`. Because [`DEV`] is a `const`, a release build folds the branch
/// away and drops `body` with it, including any message strings and bookkeeping it alone
/// references.
///
/// `body` is type-checked in every mode. That is the point: a diagnostic that only compiles on
/// one profile rots, and the rot surfaces as a broken release build. The cost is that `body` may
/// not reference items that themselves exist only in a dev build.
///
/// Control flow escapes the block on one profile only. A `return`, `?`, `break`, or `continue`
/// inside `body` runs in a dev build and is skipped entirely in a release build, so the
/// surrounding function must be correct when the block does nothing. Confine `body` to
/// diagnostics and their bookkeeping; if the enclosing function's result depends on it, the
/// profiles disagree.
///
/// # Examples
///
/// ```
/// use retroglyph_core::dev_only;
///
/// # fn warn_overflow(_: (u32, u32), _: (u32, u32)) {}
/// let sprite_px = (32, 32);
/// let cell_px = (16, 16);
///
/// dev_only!({
///     if sprite_px > cell_px {
///         warn_overflow(sprite_px, cell_px);
///     }
/// });
/// ```
///
/// The block form is not required; any statements work.
///
/// ```
/// # use retroglyph_core::dev_only;
/// # let mut misses = 0;
/// dev_only!(misses += 1;);
/// ```
#[macro_export]
macro_rules! dev_only {
    ($($body:tt)*) => {
        if $crate::dev::DEV {
            $($body)*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{BuildMode, DEV};

    #[test]
    fn current_matches_dev_const() {
        assert_eq!(BuildMode::CURRENT.is_dev(), DEV);
    }

    // Tests build with `debug_assertions` on unless someone deliberately runs them under a
    // release profile, in which case the `dev` feature is what keeps this true.
    #[test]
    fn tests_run_in_a_reporting_build() {
        assert_eq!(
            DEV,
            cfg!(debug_assertions) || cfg!(feature = "dev"),
            "BuildMode::CURRENT should track debug_assertions and the `dev` feature"
        );
    }

    #[test]
    fn dev_only_body_runs_iff_dev() {
        let mut ran = false;
        dev_only!({
            ran = true;
        });
        assert_eq!(ran, DEV);
    }

    #[test]
    fn dev_only_accepts_bare_statements() {
        let mut n = 0;
        dev_only!(n += 1;);
        assert_eq!(n, i32::from(DEV));
    }

    /// Mirrors the `warn_sprite_needs_span`-style shape: a `dev_only!` body that returns early.
    /// Pins that the early return only happens in a dev build, and that a release build falls
    /// through to the caller's own trailing value instead.
    fn returns_early_in_dev() -> bool {
        dev_only!({
            return true;
        });
        false
    }

    #[test]
    fn dev_only_early_return_runs_iff_dev() {
        assert_eq!(returns_early_in_dev(), DEV);
    }
}
