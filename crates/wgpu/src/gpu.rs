//! Device acquisition and the window surface.
//!
//! [`GpuContext`] is the adapter/device/queue triple, created once and shared by every pipeline.
//! [`WindowSurface`] adds the swap chain for a window; the offscreen render tests build a
//! [`GpuContext`] with no [`WindowSurface`] at all, which is why the two are separate types.
//!
//! # Color space
//!
//! The one non-obvious choice here is [`WindowSurface::view_format`]. `retroglyph` resolves cell colors
//! to `u8` RGB and expects those exact bytes on screen: `retroglyph-software` writes them into a
//! pixel buffer verbatim, and this backend must agree with it, since the two are checked against
//! each other pixel-for-pixel (see `crate::headless`). Rendering into an sRGB-suffixed surface
//! format would break that: the GPU encodes linear-to-sRGB on write, so a shader emitting
//! `0x80 / 255.0` would land as roughly `0xBC`, uniformly brightening every frame relative to the
//! CPU rasterizer.
//!
//! So the surface is always *viewed* through a non-sRGB (`*Unorm`) format. If the platform offers
//! one directly it is used as the surface format; if every supported format carries the sRGB
//! suffix, the surface keeps its preferred format and the non-sRGB sibling is registered in
//! `view_formats`, which is exactly what that field is for.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary is
// intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

use crate::error::SurfaceError;
use retroglyph_window::WindowHandle;
use std::sync::Arc;

/// The wgpu instance, adapter, device, and queue.
///
/// Created once per [`init_surface`](retroglyph_window::Presenter::init_surface). The `wgpu`
/// handles are all reference-counted internally, so cloning one out to pass around is cheap;
/// this type exists to keep them together and to own the instance, which the surface's lifetime
/// depends on.
pub(crate) struct GpuContext {
    /// Held for as long as the device is, purely as a lifetime anchor: a surface and an adapter
    /// are both created from the instance, and keeping it alive alongside them avoids depending on
    /// whichever internal reference counting `wgpu` happens to use. Never read, hence the
    /// underscore.
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl GpuContext {
    /// Requests an adapter and device with no surface to be compatible with, for offscreen
    /// rendering.
    ///
    /// Test-only: the offscreen path exists to run the real pipeline into a texture the tests own
    /// (see `crate::headless`). Exposing it publicly would mean also exposing a readback API and a
    /// target-format choice, neither of which a windowed backend's callers need.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if no adapter matches the enabled backends, or if the
    /// adapter can't provide a device at the limits [`request_device`] asks for.
    #[cfg(all(test, feature = "default-font"))]
    pub(crate) fn offscreen() -> Result<Self, SurfaceError> {
        let instance = create_instance();
        let adapter = request_adapter(&instance, None)?;
        let (device, queue) = request_device(&adapter)?;
        Ok(Self {
            _instance: instance,
            adapter,
            device,
            queue,
        })
    }

    /// Requests an adapter and device that can present to `window`, and returns both alongside the
    /// configured swap chain.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if the window's handles don't name a surface any enabled
    /// backend supports, if no adapter can present to it, or if the device request fails.
    pub(crate) fn windowed(
        window: Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
    ) -> Result<(Self, WindowSurface), SurfaceError> {
        let instance = create_instance();
        // `SurfaceTarget::DisplayAndWindow` hands wgpu both raw handles and takes ownership of the
        // `Arc`, so the surface keeps the window alive for its own lifetime and comes back with a
        // `'static` lifetime. That is what lets this be safe code: the borrowing alternative
        // (`create_surface_unsafe`) puts the outlives requirement on the caller instead.
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::DisplayAndWindow(Box::new(window)))
            .map_err(|e| SurfaceError::Init(format!("create surface: {e}")))?;
        let adapter = request_adapter(&instance, Some(&surface))?;
        let (device, queue) = request_device(&adapter)?;

        let gpu = Self {
            _instance: instance,
            adapter,
            device,
            queue,
        };
        let surface = WindowSurface::new(surface, &gpu, width, height)?;
        Ok((gpu, surface))
    }

    /// A one-line description of the adapter this device came from, for the init log line.
    pub(crate) fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
    }
}

/// A window's swap chain, plus the format its frames are rendered through.
pub(crate) struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// The non-sRGB format every frame view and every render pipeline uses. See the module docs.
    pub(crate) view_format: wgpu::TextureFormat,
}

impl WindowSurface {
    /// Configures `surface` for `width` x `height`, choosing the non-sRGB view format.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if the adapter reports no supported format for this surface,
    /// which means the two are not compatible at all.
    fn new(
        surface: wgpu::Surface<'static>,
        gpu: &GpuContext,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfaceError> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let preferred = *caps.formats.first().ok_or_else(|| {
            SurfaceError::Init("adapter supports no format for this surface".to_string())
        })?;
        // Prefer a format that is already non-sRGB; fall back to viewing the preferred one through
        // its non-sRGB sibling.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(preferred);
        let view_format = format.remove_srgb_suffix();
        let view_formats = if view_format == format {
            Vec::new()
        } else {
            vec![view_format]
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            // `Auto` is the only value guaranteed supported for every format an adapter reports;
            // an explicit space must appear in that format's own `color_spaces` set or `configure`
            // fails validation. For the 8-bit formats here it resolves to sRGB anyway.
            color_space: wgpu::SurfaceColorSpace::Auto,
            // A zero-size surface is invalid; a window can legitimately report one while
            // minimized, so clamp rather than fail.
            width: width.max(1),
            height: height.max(1),
            // `Fifo` is the only mode WebGPU guarantees, and it is what a grid renderer wants:
            // frames are cheap, so tearing to shave latency buys nothing.
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats,
        };
        surface.configure(&gpu.device, &config);
        Ok(Self {
            surface,
            config,
            view_format,
        })
    }

    /// Reconfigures the swap chain for a new physical pixel size. A zero dimension is clamped to
    /// one, since a window can report a zero-size client area while minimized.
    pub(crate) fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (self.config.width, self.config.height) == (width, height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Acquires the next frame, reconfiguring once and retrying if the swap chain went stale.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Frame`] for a timed-out or occluded frame, or one still stale after
    /// a reconfigure, and [`SurfaceError::DeviceLost`] when the surface reports a lost device or a
    /// validation failure, neither of which reconfiguring can fix.
    pub(crate) fn acquire(&self, gpu: &GpuContext) -> Result<Frame, SurfaceError> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => Ok(self.frame(texture)),
            // Suboptimal frames are renderable; the reconfigure is only a hint, so take the frame
            // and apply the hint on the next acquire.
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                self.surface.configure(&gpu.device, &self.config);
                Ok(self.frame(texture))
            }
            // Outdated is the ordinary consequence of a resize racing a frame: reconfigure and
            // take the frame the new swap chain hands back.
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&gpu.device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(texture)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Ok(self.frame(texture)),
                    other => Err(SurfaceError::Frame(format!(
                        "still {other:?} after reconfiguring"
                    ))),
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                Err(SurfaceError::Frame("timed out".to_string()))
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                Err(SurfaceError::Frame("window occluded".to_string()))
            }
            wgpu::CurrentSurfaceTexture::Lost => Err(SurfaceError::DeviceLost(
                "surface lost; the device and its resources must be rebuilt".to_string(),
            )),
            wgpu::CurrentSurfaceTexture::Validation => Err(SurfaceError::DeviceLost(
                "surface acquire raised a validation error".to_string(),
            )),
        }
    }

    /// Wraps an acquired swap-chain texture in a view using the non-sRGB [`Self::view_format`].
    fn frame(&self, texture: wgpu::SurfaceTexture) -> Frame {
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("retroglyph frame"),
            format: Some(self.view_format),
            ..Default::default()
        });
        Frame { texture, view }
    }
}

/// One acquired swap-chain frame and the view to render into it through.
pub(crate) struct Frame {
    texture: wgpu::SurfaceTexture,
    pub(crate) view: wgpu::TextureView,
}

impl Frame {
    /// Hands the frame back to the compositor. Consumes the frame, since the view borrows the
    /// texture that presenting gives away.
    pub(crate) fn present(self, gpu: &GpuContext) {
        drop(self.view);
        gpu.queue.present(self.texture);
    }
}

/// A wgpu instance over the backends this crate enables, honoring `WGPU_BACKEND`.
fn create_instance() -> wgpu::Instance {
    // `from_env` reads `WGPU_BACKEND`, which is how the offscreen tests pin themselves to a
    // software rasterizer in CI. Falling back to every compiled-in backend keeps ordinary use
    // configuration-free.
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::from_env().unwrap_or_else(wgpu::Backends::all);
    desc.flags = wgpu::InstanceFlags::from_build_config().with_env();
    wgpu::Instance::new(desc)
}

/// Requests an adapter, preferring low power: a character grid is a handful of draw calls, so a
/// discrete GPU would spin up for nothing on a laptop.
fn request_adapter(
    instance: &wgpu::Instance,
    compatible_surface: Option<&wgpu::Surface<'static>>,
) -> Result<wgpu::Adapter, SurfaceError> {
    let options = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::from_env()
            .unwrap_or(wgpu::PowerPreference::LowPower),
        force_fallback_adapter: false,
        compatible_surface,
        apply_limit_buckets: false,
    };
    pollster::block_on(instance.request_adapter(&options))
        .map_err(|e| SurfaceError::Init(format!("no suitable adapter: {e}")))
}

/// Requests a device at downlevel limits, so the backend runs on the weakest hardware wgpu
/// supports rather than only on whatever the developer's machine happens to be.
fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), SurfaceError> {
    // `downlevel_defaults` is the conservative floor (256 array layers, 4 bind groups, 2048px
    // textures). `using_resolution` raises just the texture/buffer *size* limits back to what this
    // adapter actually offers, so a large grid on a large display isn't capped by a floor that
    // exists for feature portability, not for capacity.
    let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("retroglyph-wgpu"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
        .map_err(|e| SurfaceError::Init(format!("request device: {e}")))?;
    // Validation and out-of-memory errors are asynchronous. Without a handler wgpu panics on the
    // first one, taking the whole app down; logging instead keeps a shipped game running (and
    // usually still drawing) while making the failure findable.
    device.on_uncaptured_error(Arc::new(|error| {
        log::error!("wgpu: {error}");
    }));
    Ok((device, queue))
}
