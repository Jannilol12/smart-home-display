use core::convert::Infallible;
use std::rc::Rc;

use embedded_graphics::{
    mono_font::{
        iso_8859_1::{FONT_6X10, FONT_9X15, FONT_9X15_BOLD},
        MonoTextStyleBuilder,
    },
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

use super::{pull_weather, DisplayModule, UpdateCtx};
use crate::helpers::icons::{self, IconKind};
use crate::helpers::weather_data::WeatherData;
use crate::helpers::{draw_panel, glyphs, weekday};

/// Bottom-left block: the next three days at a glance.
pub struct DailyForecastModule {
    bounds: Rectangle,
    url: String,
    data: Option<Rc<WeatherData>>,
}

impl DailyForecastModule {
    pub fn new(bounds: Rectangle, url: String) -> Self {
        Self {
            bounds,
            url,
            data: None,
        }
    }
}

impl DisplayModule for DailyForecastModule {
    fn bounds(&self) -> Rectangle {
        self.bounds
    }

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
        let inner = draw_panel(display, self.bounds, "3-DAY FORECAST")?;
        let ix = inner.top_left.x;
        let iy = inner.top_left.y;
        let iw = inner.size.width as i32;
        let ih = inner.size.height as i32;

        let bold = MonoTextStyleBuilder::new()
            .font(&FONT_9X15_BOLD)
            .text_color(Color::Black)
            .build();
        let mid = MonoTextStyleBuilder::new()
            .font(&FONT_9X15)
            .text_color(Color::Black)
            .build();
        let small = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(Color::Black)
            .build();
        let center = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Center)
            .build();
        let left = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let stroke1 = PrimitiveStyle::with_stroke(Color::Black, 1);

        let Some(data) = &self.data else {
            Text::with_text_style("Loading...", Point::new(ix, iy + 20), mid, center)
                .draw(display)?;
            return Ok(());
        };
        let Some(daily) = &data.daily else {
            Text::with_text_style("No data", Point::new(ix, iy + 20), mid, center).draw(display)?;
            return Ok(());
        };

        // Skip today; show the next three days.
        let days = daily.time.len().saturating_sub(1).min(3);
        let colw = iw / 3;

        for c in 0..days {
            let idx = c + 1;
            let cx0 = ix + c as i32 * colw;
            let cx = cx0 + colw / 2;

            if c > 0 {
                Line::new(Point::new(cx0, iy), Point::new(cx0, iy + ih - 4))
                    .draw_styled(&stroke1, display)?;
            }

            let label = weekday(&daily.time[idx]);
            Text::with_text_style(label, Point::new(cx, iy), bold, center).draw(display)?;

            let code = daily.weather_code.get(idx).copied().unwrap_or(3);
            icons::draw(
                display,
                Point::new(cx - 19, iy + 13),
                38,
                IconKind::from_wmo(code),
            )?;

            let tmax = daily.temperature_2m_max.get(idx).copied().unwrap_or(0.0);
            let tmin = daily.temperature_2m_min.get(idx).copied().unwrap_or(0.0);
            Text::with_text_style(
                &format!("{:.0}\u{00b0}/{:.0}\u{00b0}", tmax, tmin),
                Point::new(cx, iy + 52),
                mid,
                center,
            )
            .draw(display)?;

            // Line 1: rain probability + rain amount (mm), each with an icon.
            let prob = daily
                .precipitation_probability_max
                .get(idx)
                .and_then(|p| *p)
                .unwrap_or(0);
            let mm = daily.precipitation_sum.get(idx).copied().unwrap_or(0.0);
            let gy1 = iy + 70;
            glyphs::rain(display, Point::new(cx0 + 8, gy1), 12)?;
            Text::with_text_style(
                &format!("{}%", prob),
                Point::new(cx0 + 22, gy1 + 1),
                small,
                left,
            )
            .draw(display)?;
            glyphs::millimeters(display, Point::new(cx0 + 60, gy1), 12)?;
            Text::with_text_style(
                &format!("{:.1}mm", mm),
                Point::new(cx0 + 74, gy1 + 1),
                small,
                left,
            )
            .draw(display)?;

            // Line 2: sunshine hours with an icon.
            let sun_h = daily
                .sunshine_duration
                .get(idx)
                .map(|s| s / 3600.0)
                .unwrap_or(0.0);
            let gy2 = iy + 84;
            glyphs::sunshine(display, Point::new(cx0 + 34, gy2), 12)?;
            Text::with_text_style(
                &format!("{:.0}h", sun_h),
                Point::new(cx0 + 48, gy2 + 1),
                small,
                left,
            )
            .draw(display)?;
        }

        Ok(())
    }
}
