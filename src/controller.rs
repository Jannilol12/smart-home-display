use crate::display::DisplayHardware;
use crate::modules::DisplayModule;
use embedded_graphics::prelude::*;
use epd_waveshare::color::Color;
use epd_waveshare::epd7in5_v2::Display7in5;
use epd_waveshare::prelude::WaveshareDisplay;
use std::alloc::{alloc_zeroed, Layout};

pub struct DisplayController<'a> {
    hardware: DisplayHardware<'a>,
    frame_buffer: Box<Display7in5>,
}

impl<'a> DisplayController<'a> {
    pub fn new(hardware: DisplayHardware<'a>) -> Self {
        // Safely allocate the 48KB directly into the Heap, bypassing the stack entirely!
        let mut frame_buffer = unsafe {
            let layout = Layout::new::<Display7in5>();
            let ptr = alloc_zeroed(layout) as *mut Display7in5;
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            Box::from_raw(ptr)
        };

        let _ = frame_buffer.clear(Color::White);

        Self {
            hardware,
            frame_buffer,
        }
    }

    /// Composite every module onto a fresh white frame and push it to the panel.
    pub fn render(&mut self, modules: &[&dyn DisplayModule]) {
        self.frame_buffer
            .clear(Color::White)
            .expect("Failed to clear frame buffer");

        for module in modules {
            module
                .render(&mut self.frame_buffer)
                .expect("Failed to render module");
        }

        self.hardware
            .epd
            .wake_up(&mut self.hardware.spi_device, &mut self.hardware.delay)
            .expect("Failed to wake up EPD");

        // This panel renders with inverted polarity: a `Color::White` cleared
        // buffer would show up as a black background. Invert every byte in place
        // so the intended black-on-white result reaches the screen, while all
        // the module drawing code keeps its natural "clear white, draw black"
        // logic.
        //
        // We deliberately do NOT collect into a second `Vec` here. Allocating
        // another 48 KB framebuffer on every refresh aborts with an
        // out-of-memory error once the heap is fragmented by the Wi-Fi/TLS/HTTP
        // work earlier in the loop. The next render clears the buffer again, so
        // the in-place inversion never has to be undone.
        //
        // SAFETY: `&mut self` gives us exclusive access to `frame_buffer`, but
        // the driver's `Display` type only exposes a shared `buffer()`
        // accessor. We reborrow those bytes mutably to flip them; no other
        // reference to the buffer is alive while we write.
        {
            let bytes = self.frame_buffer.buffer();
            let buf =
                unsafe { core::slice::from_raw_parts_mut(bytes.as_ptr() as *mut u8, bytes.len()) };
            for b in buf.iter_mut() {
                *b = !*b;
            }
        }

        self.hardware
            .epd
            .update_and_display_frame(
                &mut self.hardware.spi_device,
                self.frame_buffer.buffer(),
                &mut self.hardware.delay,
            )
            .expect("Failed to display frame on EPD");

        self.hardware
            .epd
            .sleep(&mut self.hardware.spi_device, &mut self.hardware.delay)
            .expect("Failed to put EPD to sleep");
    }
}
