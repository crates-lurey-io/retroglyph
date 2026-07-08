//! [`rg_gallery_run!`]: generates the per-backend `main` functions for a gallery example.

/// Generates the per-backend `main` functions for a gallery example.
///
/// - `$app`: an expression producing something that implements [`App<B>`](retroglyph_core::App)
///   generically over `B: Backend`.
/// - `$title`: window title (software backend only).
/// - `$cols`/`$rows`: grid size for the software and Headless backends (crossterm fills the real
///   terminal instead).
///
/// Only one of the generated `main`s survives per build, selected by feature: `crossterm` wins
/// over `software` if both are enabled (matching `examples/runner.rs`'s backend picker); neither
/// enabled falls back to Headless.
#[macro_export]
macro_rules! rg_gallery_run {
    ($app:expr, $title:literal, $cols:expr, $rows:expr) => {
        #[cfg(feature = "crossterm")]
        fn main() -> ::std::io::Result<()> {
            ::retroglyph_crossterm::Crossterm::run($app)
        }

        #[cfg(all(feature = "software", not(feature = "crossterm")))]
        fn main() {
            #[cfg(target_arch = "wasm32")]
            ::console_error_panic_hook::set_once();

            let renderer = ::retroglyph_software::SoftwareBackendBuilder::new()
                .title($title)
                .grid_size($cols, $rows)
                .scale(2)
                .build()
                .expect("failed to initialize software backend")
                .run_headless();
            let config = ::retroglyph_window::winit::WindowConfig::fit(&renderer, $title, None);
            ::retroglyph_window::winit::run_app(config, renderer, $app).expect("event loop failed");
        }

        // Entry point the browser's wasm-bindgen glue calls to start the
        // module -- on wasm32 there's no OS process to invoke `main` for us.
        #[cfg(all(
            feature = "software",
            not(feature = "crossterm"),
            target_arch = "wasm32"
        ))]
        #[::wasm_bindgen::prelude::wasm_bindgen(start)]
        #[allow(missing_docs)]
        pub fn wasm_main() -> ::std::result::Result<(), ::wasm_bindgen::JsValue> {
            main();
            ::std::result::Result::Ok(())
        }

        #[cfg(not(any(feature = "crossterm", feature = "software")))]
        fn main() {
            $crate::run_headless($cols, $rows, $app);
        }
    };
}
