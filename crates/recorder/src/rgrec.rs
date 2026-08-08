//! `.rgrec`: the line-delimited JSON file format an [`InputRecording`] saves/loads as.
//!
//! One header line, then one line per event:
//!
//! ```text
//! {"rgrec":1,"width":80,"height":24,"created":"2026-08-08T12:00:00Z"}
//! {"delay_ms":0,"event":{"Key":{"code":{"Char":"a"},"modifiers":0,"kind":"Press","location":"Standard"}}}
//! ```
//!
//! Line-delimited rather than one JSON document: a `.rgrec` diffs and reviews like source (an
//! appended event is a one-line diff, not a reformat of the whole file), matching the reasoning
//! retroglyph#461 already applied to VHS tapes. The per-event shape falls straight out of
//! [`Event`]'s own derived [`Serialize`]/[`Deserialize`] (see `retroglyph-core`'s `serde`
//! feature); this module only owns the header and the `delay_ms`/`event` envelope around it.

use retroglyph_core::event::Event;
use retroglyph_core::testing::InputRecording;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufWriter, Write};
use std::path::Path;
use std::time::Duration;

/// The current `.rgrec` format version, written into every header's `rgrec` field.
const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Header {
    rgrec: u32,
    width: u16,
    height: u16,
    created: String,
}

#[derive(Serialize, Deserialize)]
struct EventLine {
    delay_ms: u64,
    event: Event,
}

/// Writes `recording` to `path` as `.rgrec`, overwriting any existing file.
///
/// # Errors
///
/// Returns an error if `path` cannot be created or written to.
pub fn write(recording: &InputRecording, path: impl AsRef<Path>) -> io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut out = BufWriter::new(file);

    let header = Header {
        rgrec: FORMAT_VERSION,
        width: recording.width(),
        height: recording.height(),
        created: now_rfc3339(),
    };
    write_json_line(&mut out, &header)?;

    for (delay, event) in recording.events() {
        let line = EventLine {
            #[allow(clippy::cast_possible_truncation)]
            delay_ms: delay.as_millis() as u64,
            event: event.clone(),
        };
        write_json_line(&mut out, &line)?;
    }

    out.flush()
}

/// Serializes `value` as one compact JSON line, terminated by `\n`.
fn write_json_line<W: Write, T: Serialize>(out: &mut W, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *out, value).map_err(io::Error::other)?;
    out.write_all(b"\n")
}

/// Reads a `.rgrec` file written by [`write()`] back into an [`InputRecording`].
///
/// # Errors
///
/// Returns an error if `path` cannot be opened or read, the file has no header line, or any
/// line's JSON does not parse.
pub fn read(path: impl AsRef<Path>) -> io::Result<InputRecording> {
    let file = std::fs::File::open(path)?;
    let mut lines = io::BufReader::new(file).lines();

    let header_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty .rgrec file"))??;
    let header: Header = serde_json::from_str(&header_line).map_err(io::Error::other)?;

    let mut recording = InputRecording::new(header.width, header.height);
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: EventLine = serde_json::from_str(&line).map_err(io::Error::other)?;
        recording.push(Duration::from_millis(entry.delay_ms), entry.event);
    }
    Ok(recording)
}

/// The current wall-clock time as an RFC 3339 `YYYY-MM-DDTHH:MM:SSZ` UTC timestamp, with no
/// external date/time dependency (see this crate's `Cargo.toml` for why: `.rgrec`'s header is the
/// only place a timestamp is needed at all, not worth a dependency for).
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_rfc3339(i64::try_from(secs).unwrap_or(i64::MAX))
}

/// Formats `secs` (Unix time, UTC) as `YYYY-MM-DDTHH:MM:SSZ`.
fn format_rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days` algorithm (public domain): converts a day count since the
/// Unix epoch (1970-01-01) into a `(year, month, day)` proleptic-Gregorian civil date.
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
const fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    #[allow(clippy::cast_sign_loss)]
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    #[allow(clippy::cast_possible_wrap)]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    #[allow(clippy::cast_possible_truncation)]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    #[allow(clippy::cast_possible_truncation)]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::event::{KeyCode, KeyEvent, KeyModifiers};

    fn sample_recording() -> InputRecording {
        let mut recording = InputRecording::new(80, 24);
        recording.push(
            Duration::ZERO,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        );
        recording.push(
            Duration::from_millis(125),
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
        );
        recording
    }

    #[test]
    fn write_then_read_round_trips_the_recording() {
        let dir = std::env::temp_dir().join(format!("rgrec-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("session.rgrec");

        let recording = sample_recording();
        write(&recording, &path).expect("write");
        let round_tripped = read(&path).expect("read");

        assert_eq!(round_tripped, recording);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_produces_one_header_line_and_one_line_per_event() {
        let dir = std::env::temp_dir().join(format!("rgrec-test-lines-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("session.rgrec");

        write(&sample_recording(), &path).expect("write");
        let contents = std::fs::read_to_string(&path).expect("read_to_string");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3, "one header line plus one line per event");

        let header: Header = serde_json::from_str(lines[0]).expect("parse header");
        assert_eq!(header.rgrec, FORMAT_VERSION);
        assert_eq!(header.width, 80);
        assert_eq!(header.height, 24);
        assert!(!header.created.is_empty());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_errs_on_an_empty_file() {
        let dir = std::env::temp_dir().join(format!("rgrec-test-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("empty.rgrec");
        std::fs::write(&path, "").expect("write empty file");

        let err = read(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn format_rfc3339_matches_known_instants() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_754_654_400), "2025-08-08T12:00:00Z");
    }
}
