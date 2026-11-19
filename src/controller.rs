use crate::display::DisplayHardware;
use crate::modules::DisplayModule;
use embedded_graphics::prelude::*;
use epd_waveshare::color::Color;
use epd_waveshare::epd7in5_v2::{Display7in5, WIDTH};
use epd_waveshare::prelude::{RefreshLut, WaveshareDisplay};
use std::alloc::{alloc_zeroed, Layout};

/// Capacity reserved up front for `partial_buf`. Covers the largest module
/// region of the layout (400 x 209 px = 10.4 KB) so it is allocated at boot on
/// a clean heap rather than on the first partial update.
const PARTIAL_BUF_RESERVE: usize = 12 * 1024;

pub struct DisplayController<'a> {
    hardware: DisplayHardware<'a>,
    frame_buffer: Box<Display7in5>,
    /// Reusable, byte-aligned scratch buffer holding a single module's region,
    /// packed out of `frame_buffer` for each partial update. It is reserved
    /// once and then reused, so we never allocate a fresh framebuffer-sized
    /// `Vec` per refresh (which OOMs on a fragmented heap).
    partial_buf: Vec<u8>,
    /// Set when a refresh failed midway. The panel controller may then be in an
    /// unknown state (e.g. still in partial mode, DC/DC on), so the next
    /// refresh re-initialises it with a hardware reset and redraws everything.
    needs_reset: bool,
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
            partial_buf: Vec::with_capacity(PARTIAL_BUF_RESERVE),
            needs_reset: false,
        }
    }

    /// A previous refresh failed, so the next one has to be a full redraw.
    pub fn needs_full_refresh(&self) -> bool {
        self.needs_reset
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
    /// Used for the first draw, the periodic ghosting clears and to recover
    /// from a failed refresh.
    pub fn render(&mut self, modules: &[&dyn DisplayModule]) -> anyhow::Result<()> {
        self.compose(modules);
        let result = self.push_full();
        self.finish(result)
    }

    fn push_full(&mut self) -> anyhow::Result<()> {
        let hw = &mut self.hardware;
        if self.needs_reset {
            // Hardware reset + init sequence.
            hw.epd.wake_up(&mut hw.spi_device, &mut hw.delay)?;
        }
        hw.epd.power_on(&mut hw.spi_device, &mut hw.delay)?;
        hw.epd
            .set_lut(&mut hw.spi_device, &mut hw.delay, Some(RefreshLut::Full))?;
        hw.epd.update_and_display_frame(
            &mut hw.spi_device,
            self.frame_buffer.buffer(),
            &mut hw.delay,
        )?;
        hw.epd.power_off(&mut hw.spi_device, &mut hw.delay)?;
        Ok(())
    }

    /// Re-composite the frame but push only the rectangles of the modules whose
    /// `changed[i]` is set, using the panel's flicker-free partial LUT. Each
    /// region is snapped out to 8-pixel column boundaries (the panel's partial
    /// window granularity) and packed out of the full frame buffer, so the
    /// padding pixels stay consistent with their neighbours. Falls back to a
    /// full refresh after a failed one.
    pub fn render_partial(
        &mut self,
        modules: &[&dyn DisplayModule],
        changed: &[bool],
    ) -> anyhow::Result<()> {
        if self.needs_reset {
            return self.render(modules);
        }
        self.compose(modules);
        let result = self.push_partial(modules, changed);
        self.finish(result)
    }

    fn push_partial(
        &mut self,
        modules: &[&dyn DisplayModule],
        changed: &[bool],
    ) -> anyhow::Result<()> {
        let hw = &mut self.hardware;
        hw.epd.power_on(&mut hw.spi_device, &mut hw.delay)?;
        hw.epd.set_lut(
            &mut hw.spi_device,
            &mut hw.delay,
            Some(RefreshLut::PartialRefresh),
        )?;

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

            hw.epd.update_partial_frame(
                &mut hw.spi_device,
                &mut hw.delay,
                &self.partial_buf[..region_len],
                ax,
                y0,
                aw,
                h,
            )?;
            hw.epd.display_frame(&mut hw.spi_device, &mut hw.delay)?;
        }

        hw.epd.power_off(&mut hw.spi_device, &mut hw.delay)?;
        Ok(())
    }

    /// Record the outcome of a refresh. On failure, make a best-effort attempt
    /// to switch the high-voltage DC/DC converter off (it must not stay on
    /// until the next tick) and schedule a reset + full redraw.
    fn finish(&mut self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        self.needs_reset = result.is_err();
        if self.needs_reset {
            let hw = &mut self.hardware;
            let _ = hw.epd.power_off(&mut hw.spi_device, &mut hw.delay);
        }
        result
    }
}
