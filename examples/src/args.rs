//! Command-line flag parsing for the examples gallery: `--time-scale`, `--fps`, `--headless-
//! frames`, and `--record`, parsed once from `std::env::args()` and cached for the process's
//! lifetime.
//!
//! Additive, not a replacement: `RG_TIME_SCALE`/`RG_FPS`/`RG_HEADLESS_FRAMES` keep working as
//! environment variables (see [`launch::time_scale`](crate::launch), [`crate::fps::starts_visible`],
//! [`launch::run_headless_stdout`](crate::launch::run_headless_stdout)), and a flag only overrides
//! its env var when both are set -- `examples/tests/support`'s `capture_pty_with_env`/
//! `capture_pty_animated` inject `RG_FPS`/`RG_TIME_SCALE` into spawned example children via
//! `portable-pty`'s `CommandBuilder::env` (not CLI args) for essentially every `svg_snapshot`
//! test in the gallery, so removing env-var support outright would break that whole suite.
//!
//! Hand-rolled rather than a dependency (`lexopt`/`pico-args`): four flags, all with a single
//! value, is small enough that a parsing crate would add more surface than it saves.

// `pub(crate)` on every item here is not redundant despite clippy's `nursery` lint thinking so:
// `mod args;` (`lib.rs`) is itself private, so plain `pub` would trip `unreachable_pub` instead --
// see `fps.rs`'s matching `#![allow]` for the same reason on the same shape of module.
#![allow(clippy::redundant_pub_crate)]

use std::path::PathBuf;
use std::sync::OnceLock;

/// Parsed CLI flags for this process, cached after the first
/// [`std::env::args()`](std::env::args) read.
///
/// Every field is `None` when its flag is absent *or* malformed -- the same permissive
/// "fall back, don't panic on a typo" convention `scale_from_env`
/// ([`launch`](crate::launch)) already uses for its environment-variable counterpart: this
/// exists for local debugging and asset capture, and a mistyped flag should not crash an
/// otherwise-runnable example.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Args {
    /// `--time-scale <factor>`: see [`launch::time_scale`](crate::launch).
    pub time_scale: Option<f64>,
    /// `--fps <on|off>`: see [`crate::fps::starts_visible`].
    pub fps: Option<bool>,
    /// `--headless-frames <n>`: see
    /// [`launch::run_headless_stdout`](crate::launch::run_headless_stdout).
    pub headless_frames: Option<u32>,
    /// `--record <path>`: see [`launch::launch`](crate::launch).
    pub record: Option<PathBuf>,
}

impl Args {
    /// Parses `raw` (each element one `argv` entry, flag and value as separate entries: `--fps
    /// off`, not `--fps=off`) into an [`Args`]. Unrecognized flags and bare positional arguments
    /// are ignored, not rejected: this is a demo gallery, not a CLI with a fixed contract, and a
    /// future flag this parser doesn't know about yet shouldn't make every existing example
    /// refuse to start.
    fn parse_from<I: IntoIterator<Item = String>>(raw: I) -> Self {
        let mut args = Self::default();
        let mut iter = raw.into_iter();
        while let Some(flag) = iter.next() {
            match flag.as_str() {
                "--time-scale" => {
                    args.time_scale = iter
                        .next()
                        .and_then(|value| value.trim().parse::<f64>().ok())
                        .filter(|scale| scale.is_finite() && *scale > 0.0);
                }
                "--fps" => {
                    args.fps = iter.next().map(|value| !is_off(&value));
                }
                "--headless-frames" => {
                    args.headless_frames = iter
                        .next()
                        .and_then(|value| value.trim().parse::<u32>().ok())
                        .filter(|&n| n > 0);
                }
                "--record" => {
                    args.record = iter.next().map(PathBuf::from);
                }
                _ => {}
            }
        }
        args
    }
}

/// Matches the same `"0"`/`"false"`/`"off"` (case-insensitive, trimmed) convention
/// [`crate::fps`]'s own `visible_from_env` uses, so `--fps off` and `RG_FPS=off` mean the same
/// thing.
fn is_off(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off"
    )
}

/// This process's parsed [`Args`], read from [`std::env::args()`](std::env::args) once and
/// cached -- every call site (`time_scale`, `fps::starts_visible`, `run_headless_stdout`,
/// `launch::launch`) reads the same parse rather than re-scanning `argv` per call.
///
/// Always empty on `wasm32` (nothing passes `argv` there, the same way nothing sets environment
/// variables there).
pub(crate) fn parsed() -> &'static Args {
    static ARGS: OnceLock<Args> = OnceLock::new();
    ARGS.get_or_init(|| {
        #[cfg(target_arch = "wasm32")]
        {
            Args::default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Args::parse_from(std::env::args().skip(1))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(raw: &[&str]) -> Args {
        Args::parse_from(raw.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn no_flags_leaves_every_field_none() {
        assert_eq!(args(&[]), Args::default());
    }

    #[test]
    fn parses_a_positive_time_scale() {
        assert_eq!(args(&["--time-scale", "20"]).time_scale, Some(20.0));
        assert_eq!(args(&["--time-scale", " 2.5 "]).time_scale, Some(2.5));
    }

    #[test]
    fn rejects_a_time_scale_that_is_not_a_usable_speed() {
        for value in ["", "fast", "0", "-1", "inf", "NaN"] {
            assert_eq!(
                args(&["--time-scale", value]).time_scale,
                None,
                "{value:?} should not have parsed"
            );
        }
    }

    #[test]
    fn a_flag_with_no_following_value_is_ignored() {
        assert_eq!(args(&["--time-scale"]).time_scale, None);
        assert_eq!(args(&["--record"]).record, None);
    }

    #[test]
    fn fps_on_off_and_synonyms() {
        assert_eq!(args(&["--fps", "off"]).fps, Some(false));
        assert_eq!(args(&["--fps", "0"]).fps, Some(false));
        assert_eq!(args(&["--fps", "false"]).fps, Some(false));
        assert_eq!(args(&["--fps", "on"]).fps, Some(true));
        assert_eq!(args(&["--fps", "anything-else"]).fps, Some(true));
    }

    #[test]
    fn parses_headless_frames() {
        assert_eq!(args(&["--headless-frames", "7"]).headless_frames, Some(7));
        assert_eq!(args(&["--headless-frames", "0"]).headless_frames, None);
        assert_eq!(args(&["--headless-frames", "nope"]).headless_frames, None);
    }

    #[test]
    fn parses_record_path() {
        assert_eq!(
            args(&["--record", "docs/assets/demo.cast"]).record,
            Some(PathBuf::from("docs/assets/demo.cast"))
        );
    }

    #[test]
    fn unrecognized_flags_are_ignored() {
        assert_eq!(
            args(&["--not-a-flag", "value", "--time-scale", "5"]).time_scale,
            Some(5.0)
        );
    }
}
