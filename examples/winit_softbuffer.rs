use retroglyph::{
    core::{Cell, Font, Grid},
    grid,
    render::{self, Buffer},
};
use softbuffer::{Context, Surface};
use std::{num::NonZeroU32, rc::Rc};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

fn main() {
    let font = Font::IBM_VGA_8X8;
    let mut output = grid!(80, 25);

    // Loop through every glyph and fill the output grid with it
    #[allow(clippy::cast_possible_truncation)]
    for (i, cell) in output.iter_mut().enumerate() {
        // Wrap the index in a `u8` to get the glyph
        let glyph_index = (i % 256) as u8;

        // Set the cell to the glyph at the index
        *cell = Cell::new(glyph_index);
    }

    let mut app = App::<2000> {
        display: None,
        output,
        font,
    };

    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut app).unwrap();
}

struct App<const LENGTH: usize> {
    display: Option<Display>,
    output: Grid<LENGTH>,
    font: Font,
}

struct Display {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
}

impl Display {
    fn new(window: &Rc<Window>) -> Self {
        let context = Context::new(window.clone()).unwrap();
        let surface = Surface::new(&context, window.clone()).unwrap();
        Self {
            window: window.clone(),
            surface,
        }
    }
}

impl<const LENGTH: usize> ApplicationHandler for App<LENGTH> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = make_window(event_loop, |attrs| attrs);
        window.request_redraw();
        self.display = Some(Display::new(&Rc::new(window)));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                let Some(display) = &mut self.display else {
                    eprintln!("Display not initialized");
                    return;
                };
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    display.surface.resize(width, height).unwrap();
                    display.window.request_redraw();
                }
            }
            WindowEvent::CloseRequested => {
                eprintln!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let Some(display) = &mut self.display else {
                    eprintln!("Display not initialized");
                    return;
                };
                let size = display.window.inner_size();
                if let Some(width) = NonZeroU32::new(size.width) {
                    let mut frame = display.surface.buffer_mut().unwrap();
                    frame.fill(0xFF00_0000);

                    // Create a Buffer with the correct dimensions
                    let mut buffer = Buffer::from_argb(&mut frame, width.get() as usize);

                    // Calculate the largest scale that fits the window
                    #[allow(clippy::cast_possible_truncation)]
                    let scale_x = size.width / (self.output.width() as u32 * 8);

                    #[allow(clippy::cast_possible_truncation)]
                    let scale_y = size.height / (self.output.height() as u32 * 8);
                    let scale = scale_x.min(scale_y).max(1) as usize;

                    // Render the output grid to the buffer
                    render::render(&self.output, &mut buffer, &self.font, scale);
                    frame.present().unwrap();
                }
            }
            _ => {}
        }
    }
}

fn make_window(
    elwt: &ActiveEventLoop,
    f: impl FnOnce(WindowAttributes) -> WindowAttributes,
) -> Window {
    let attributes = f(WindowAttributes::default());
    let (width, height) = (80.0 * 8.0, 25.0 * 8.0);
    let attributes = attributes
        .with_title("Retroglyph Demo")
        .with_resizable(true)
        .with_min_inner_size(winit::dpi::LogicalSize::new(width, height))
        .with_inner_size(winit::dpi::LogicalSize::new(width * 2.0, height * 2.0));
    elwt.create_window(attributes).unwrap()
}
