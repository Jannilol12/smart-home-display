use core::convert::Infallible;
use std::rc::Rc;

use embedded_graphics::{
    mono_font::{iso_8859_1::FONT_6X10, MonoTextStyleBuilder},
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

use crate::helpers::draw_panel;
use crate::helpers::weather_data::WeatherData;
use super::{pull_weather, DisplayModule, UpdateCtx};

/// Top-right block: a compact 24-hour chart combining the temperature trend
/// (line), rain probability (outline bars) and rain amount (filled bars).
pub struct ForecastGraphModule {
    bounds: Rectangle,
    url: String,
    data: Option<Rc<WeatherData>>,
}

impl ForecastGraphModule {
    pub fn new(bounds: Rectangle, url: String) -> Self {
        Self {
            bounds,
            url,
            data: None,
        }
    }
}

impl DisplayModule for ForecastGraphModule {
    fn update(&mut self, ctx: &mut UpdateCtx) -> bool {
        match pull_weather(ctx, &self.url) {
            Some(wd) => {
                self.data = Some(wd);
                true
            }
            None => false,
        }
    }

    fn render(&self, display: &mut Display7in5) -> Result<(), Infallible> {
        let inner = draw_panel(display, self.bounds, "NEXT 24 HOURS  (line \u{00b0}C - bars rain% - fill mm)")?;
        let ix = inner.top_left.x;
        let iy = inner.top_left.y;
        let iw = inner.size.width as i32;
        let ih = inner.size.height as i32;

        let small = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(Color::Black)
            .build();
        let top_left = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let top_center = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Center)
            .build();
        let stroke1 = PrimitiveStyle::with_stroke(Color::Black, 1);
        let stroke2 = PrimitiveStyle::with_stroke(Color::Black, 2);
        let fill = PrimitiveStyle::with_fill(Color::Black);

        let Some(data) = &self.data else {
            Text::with_text_style("Loading...", Point::new(ix, iy + 20), small, top_left)
                .draw(display)?;
            return Ok(());
        };

        let start = data.current_hour_index();
        let temps = &data.hourly.temperature_2m;
        let probs = &data.hourly.precipitation_probability;
        let rains = &data.hourly.rain;
        let times = &data.hourly.time;

        let avail = temps.len().saturating_sub(start);
        let n = avail.min(24);
        if n < 2 {
            Text::with_text_style("No data", Point::new(ix, iy + 20), small, top_left)
                .draw(display)?;
            return Ok(());
        }

        // Vertical split: temperature chart on top, precipitation below. The
        // separator line doubles as the precipitation full-scale mark (100% /
        // peak mm) so a ~95% bar visibly reaches almost up to the line.
        let px0 = ix + 24;
        let px1 = ix + iw - 4;
        let temp_top = iy + 2;
        let sep = iy + (ih as f32 * 0.46) as i32;
        let prec_bot = iy + ih - 12;
        let step = (px1 - px0) as f32 / (n - 1) as f32;

        // ---- Temperature line ----
        let slice: Vec<f32> = temps[start..start + n].to_vec();
        let min_t = slice.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_t = slice.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let range = (max_t - min_t).max(1.0);
        let plot_h = (sep - temp_top - 4) as f32;

        Text::with_text_style(
            &format!("{:.0}\u{00b0}", max_t),
            Point::new(ix, temp_top),
            small,
            top_left,
        )
        .draw(display)?;
        Text::with_text_style(
            &format!("{:.0}\u{00b0}", min_t),
            Point::new(ix, sep - 10),
            small,
            top_left,
        )
        .draw(display)?;
        Line::new(Point::new(px0, sep), Point::new(px1, sep))
            .draw_styled(&stroke1, display)?;

        let pts: Vec<Point> = slice
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let x = px0 + (i as f32 * step) as i32;
                let y = sep - 2 - ((t - min_t) / range * plot_h) as i32;
                Point::new(x, y)
            })
            .collect();
        for w in pts.windows(2) {
            Line::new(w[0], w[1]).draw_styled(&stroke2, display)?;
        }

        // ---- Precipitation: probability (outline bars) + rain mm (filled) ----
        // Left axis = probability %, right axis = rain amount; both peak at the
        // separator line (= 100% and the window's max mm).
        let prec_h = (prec_bot - sep) as f32;
        let rain_max = rains[start..start + n]
            .iter()
            .cloned()
            .fold(0.0_f32, f32::max);
        let rain_scale = rain_max.max(0.5);
        Line::new(Point::new(px0, prec_bot), Point::new(px1, prec_bot))
            .draw_styled(&stroke1, display)?;
        let scale_label = if rain_max > 0.0 {
            format!("100%/{:.1}mm", rain_max)
        } else {
            "100%".to_string()
        };
        Text::with_text_style(&scale_label, Point::new(ix, sep + 1), small, top_left).draw(display)?;

        let bar_w = (step * 0.6).max(3.0) as u32;
        for i in 0..n {
            let cx = px0 + (i as f32 * step) as i32;
            let bx = cx - bar_w as i32 / 2;

            if let Some(Some(p)) = probs.get(start + i) {
                let ph = (*p as f32 / 100.0 * prec_h) as i32;
                if ph > 0 {
                    Rectangle::new(
                        Point::new(bx, prec_bot - ph),
                        Size::new(bar_w, ph as u32),
                    )
                    .draw_styled(&stroke1, display)?;
                }
            }

            if let Some(mm) = rains.get(start + i) {
                if *mm > 0.0 {
                    let rh = ((*mm / rain_scale) * prec_h) as i32;
                    let fw = (bar_w / 2).max(2);
                    let fx = cx - fw as i32 / 2;
                    if rh > 0 {
                        Rectangle::new(
                            Point::new(fx, prec_bot - rh),
                            Size::new(fw, rh as u32),
                        )
                        .draw_styled(&fill, display)?;
                    }
                }
            }
        }

        // ---- X axis hour labels (every 3rd hour) ----
        for i in (0..n).step_by(3) {
            let cx = px0 + (i as f32 * step) as i32;
            let hh = times
                .get(start + i)
                .and_then(|t| t.get(11..13))
                .unwrap_or("--");
            Line::new(
                Point::new(cx, prec_bot),
                Point::new(cx, prec_bot + 3),
            )
            .draw_styled(&stroke1, display)?;
            Text::with_text_style(hh, Point::new(cx, prec_bot + 2), small, top_center)
                .draw(display)?;
        }

        Ok(())
    }
}
