//! [`install_panic_recorder`]: auto-save an in-progress recording on panic.

use crate::RecorderHandle;
use std::path::PathBuf;

/// Installs a panic hook that saves whatever `handle`'s recorder has captured so far to `path`
/// when the process panics, then chains to whatever hook was already installed.
///
/// The saved recording is exactly what a bug report needs: the input stream leading up to the
/// crash, ready to hand to [`TestHarness::replay`](retroglyph_core::testing::TestHarness::replay)
/// or [`replay_live`](crate::replay_live) as a regression test, with no hand-transcription.
///
/// Chaining to the previous hook (rather than replacing it) is deliberate: this must never
/// suppress the panic message itself, or any other diagnostic (e.g. `color-backtrace`,
/// `console_error_panic_hook`) a consumer already installed. The save happens first, so a panic
/// *while* saving (a full disk, a bad `path`) still lets the original panic's own output reach
/// the terminal via the chained hook, rather than replacing it with a save failure.
///
/// This installs no hotkey and has no opinion on what triggers a save beyond an actual panic:
/// deliberate, so it can never collide with a consumer's own keybindings. Call
/// [`RecorderHandle::save`] directly wherever a consumer wants a non-panic trigger (a dedicated
/// debug key, a signal handler, an explicit "report a bug" menu item).
///
/// # Examples
///
/// ```no_run
/// use retroglyph_core::backend::Headless;
/// use retroglyph_recorder::{InputRecorder, install_panic_recorder};
///
/// let recorder = InputRecorder::new(Headless::new(80, 24));
/// install_panic_recorder(recorder.handle(), "crash.rgrec");
/// ```
pub fn install_panic_recorder(handle: RecorderHandle, path: impl Into<PathBuf>) {
    let path = path.into();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort: a save failure here (a full disk, an unwritable path) must not stop the
        // original panic's own output from reaching the terminal via `previous` below.
        let _ = handle.save(&path);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InputRecorder;
    use retroglyph_core::backend::{Headless, Input};
    use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    /// Runs `body` with `install_panic_recorder` active, catching the panic so the test process
    /// itself survives, and returns the path the recording was saved to.
    ///
    /// Panic hooks are process-global, so this test (and any other exercising a panic hook in
    /// this crate) must not run concurrently with one that installs its own; `#[test]`s in one
    /// binary share a process but not a thread, and `std::panic::set_hook` applies process-wide.
    /// Serializing isn't done here explicitly (this module has only the one hook-installing
    /// test), but a future addition should keep that in mind.
    #[test]
    fn saves_the_recording_on_panic_without_suppressing_the_original_panic() {
        let dir =
            std::env::temp_dir().join(format!("rgrec-panic-hook-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("crash.rgrec");
        std::fs::remove_file(&path).ok();

        let mut recorder = InputRecorder::new(Headless::new(4, 1));
        recorder.push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));
        recorder.poll_event(std::time::Duration::ZERO);

        let previous_hook_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = previous_hook_ran.clone();
        std::panic::set_hook(Box::new(move |_| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        install_panic_recorder(recorder.handle(), path.clone());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panic!("boom");
        }));
        assert!(result.is_err());

        // Restore Rust's default hook so later tests in this binary aren't affected by the one
        // this test just installed.
        let _ = std::panic::take_hook();

        assert!(
            previous_hook_ran.load(std::sync::atomic::Ordering::SeqCst),
            "the previously installed hook must still run, so the original panic is never suppressed"
        );
        let saved = crate::read(&path).expect("saved recording should be readable");
        assert_eq!(saved.len(), 1);

        std::fs::remove_file(&path).ok();
    }
}
