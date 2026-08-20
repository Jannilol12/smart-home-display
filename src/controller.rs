use crate::display::DisplayHardware;
use crate::modules::DisplayModule;
use embedded_graphics::prelude::*;
use epd_waveshare::color::Color;
use epd_waveshare::epd7in5_v2::Display7in5;
use epd_waveshare::prelude::WaveshareDisplay;
use std::alloc::{alloc_zeroed, Layout};

pub struct DisplayController<'a> {
    modules: Vec<Box<dyn DisplayModule<Display7in5>>>,
    active_index: usize,
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
            modules: Vec::new(),
            active_index: 0,
            hardware,
            frame_buffer,
        }
    }

    pub fn register(&mut self, module: Box<dyn DisplayModule<Display7in5>>) {
        self.modules.push(module);
    }

    pub fn clear_modules(&mut self) {
        self.modules.clear();
        self.active_index = 0;
    }

    pub fn force_render(&mut self) {
        if self.modules.is_empty() {
            return;
        }

        println!(
            "Rendering module: {}",
            self.modules[self.active_index].name()
        );

        self.frame_buffer
            .clear(Color::White)
            .expect("Failed to clear frame buffer");

        self.modules[self.active_index]
            .render(&mut *self.frame_buffer)
            .expect("Failed to render module");

        println!("Flushing frame to E-Paper display...");
        self.hardware
            .epd
            .update_and_display_frame(
                &mut self.hardware.spi_device,
                self.frame_buffer.buffer(),
                &mut self.hardware.delay,
            )
            .expect("Failed to display frame on EPD");

        println!("E-Paper refresh completed.");
    }
}
