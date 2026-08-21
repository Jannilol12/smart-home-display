use crate::display::DisplayHardware;
use crate::modules::DisplayModule;
use embedded_graphics::prelude::*;
use epd_waveshare::color::Color;
use epd_waveshare::epd7in5_v2::{Display7in5, WIDTH};
use epd_waveshare::prelude::{RefreshLut, WaveshareDisplay};
use std::alloc::{alloc_zeroed, Layout};

pub struct DisplayController<'a> {
    hardware: DisplayHardware<'a>,
    frame_buffer: Box<Display7in5>,
    /// Reusable, byte-aligned scratch buffer holding a single module's region,
    /// packed out of `frame_buffer` for each partial update. It grows once to
    /// the largest region and is then reused, so we never allocate a fresh
    /// framebuffer-sized `Vec` per refresh (which OOMs on a fragmented heap).
    partial_buf: Vec<u8>,
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
            partial_buf: Vec::new(),
        }
    }

    /// Composite every module onto a fresh white frame, ready to flush to the
    /// panel as-is.
    fn compose(&mut self, modules: &[&dyn DisplayModule]) {
        self.frame_buffer
            .clear(Color::White)
            .expect("Failed to clear frame buffer");

        for module in modules {
            module
                .render(&mut self.frame_buffer)
                .expect("Failed to render module");
        }
    }

    /// Composite every module and push the whole frame with a full refresh.
    /// Used for the first draw and (later) periodic ghosting clears.
    pub fn render(&mut self, modules: &[&dyn DisplayModule]) {
        self.compose(modules);

        self.hardware
            .epd
            .power_on(&mut self.hardware.spi_device, &mut self.hardware.delay)
            .expect("Failed to power on EPD");

        self.hardware
            .epd
            .set_lut(
                &mut self.hardware.spi_device,
                &mut self.hardware.delay,
                Some(RefreshLut::Full),
            )
            .expect("Failed to select full LUT");

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
            .power_off(&mut self.hardware.spi_device, &mut self.hardware.delay)
            .expect("Failed to power off EPD");
    }

    /// Re-composite the frame but push only the rectangles of the modules whose
    /// `changed[i]` is set, using the panel's flicker-free partial LUT. Each
    /// region is snapped out to 8-pixel column boundaries (the panel's partial
    /// window granularity) and packed out of the full frame buffer, so the
    /// padding pixels stay consistent with their neighbours.
    pub fn render_partial(&mut self, modules: &[&dyn DisplayModule], changed: &[bool]) {
        self.compose(modules);

        self.hardware
            .epd
            .power_on(&mut self.hardware.spi_device, &mut self.hardware.delay)
            .expect("Failed to power on EPD");

        self.hardware
            .epd
            .set_lut(
                &mut self.hardware.spi_device,
                &mut self.hardware.delay,
                Some(RefreshLut::PartialRefresh),
            )
            .expect("Failed to select partial LUT");

        let stride = (WIDTH / 8) as usize;

        for (i, module) in modules.iter().enumerate() {
            if !changed.get(i).copied().unwrap_or(false) {
                continue;
            }

            let bounds = module.bounds();
            let w = bounds.size.width;
            let h = bounds.size.height;
            if w == 0 || h == 0 {
                continue;
            }
            let x0 = bounds.top_left.x.max(0) as u32;
            let y0 = bounds.top_left.y.max(0) as u32;

            // Snap x/width out to byte (8-pixel) boundaries for the partial window.
            let ax = x0 & !7;
            let ax_end = (x0 + w - 1) | 7;
            let aw = ax_end - ax + 1;
            let byte_x = (ax / 8) as usize;
            let row_bytes = (aw / 8) as usize;
            let rows = h as usize;
            let region_len = row_bytes * rows;

            // Pack the aligned region out of the composed full buffer.
            self.partial_buf.resize(region_len, 0);
            {
                let full = self.frame_buffer.buffer();
                for r in 0..rows {
                    let src = (y0 as usize + r) * stride + byte_x;
                    let dst = r * row_bytes;
                    self.partial_buf[dst..dst + row_bytes]
                        .copy_from_slice(&full[src..src + row_bytes]);
                }
            }

            self.hardware
                .epd
                .update_partial_frame(
                    &mut self.hardware.spi_device,
                    &mut self.hardware.delay,
                    &self.partial_buf[..region_len],
                    ax,
                    y0,
                    aw,
                    h,
                )
                .expect("Failed to load partial frame");

            self.hardware
                .epd
                .display_frame(&mut self.hardware.spi_device, &mut self.hardware.delay)
                .expect("Failed to refresh partial frame");
        }

        self.hardware
            .epd
            .power_off(&mut self.hardware.spi_device, &mut self.hardware.delay)
            .expect("Failed to power off EPD");
    }
}
