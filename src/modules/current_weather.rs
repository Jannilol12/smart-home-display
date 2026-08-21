use core::convert::Infallible;
use std::rc::Rc;

use embedded_graphics::{
    mono_font::{
        iso_8859_1::{FONT_10X20, FONT_6X10, FONT_9X15},
        MonoTextStyleBuilder,
    },
    prelude::*,
    primitives::Rectangle,
    text::{Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

use super::{pull_weather, DisplayModule, UpdateCtx};
use crate::helpers::icons::{self, IconKind};
use crate::helpers::weather_data::{wmo_text, WeatherData};
use crate::helpers::{compass, draw_panel, glyphs, hhmm};

/// Signature shared by every stat glyph in [`glyphs`].
type Glyph = fn(&mut Display7in5, Point, u32) -> Result<(), Infallible>;

/// Top-left block: the current conditions with an icon and key metrics.
pub struct CurrentWeatherModule {
    bounds: Rectangle,
    url: String,
    data: Option<Rc<WeatherData>>,
}

impl CurrentWeatherModule {
    pub fn new(bounds: Rectangle, url: String) -> Self {
        Self {
            bounds,
            url,
            data: None,
        }
    }
}

impl DisplayModule for CurrentWeatherModule {
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
        let inner = draw_panel(display, self.bounds, "CURRENT WEATHER")?;
        let ix = inner.top_left.x;
        let iy = inner.top_left.y;
        let iw = inner.size.width as i32;

        let top = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let big = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
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

        let Some(data) = &self.data else {
            Text::with_text_style("Loading...", Point::new(ix, iy + 20), mid, top).draw(display)?;
            return Ok(());
        };
        let cur = data.current.clone().unwrap_or_default();
        let daily = data.daily.clone().unwrap_or_default();

        // Headline: icon + big temperature, condition, feels-like + today H/L.
        let code = cur.weather_code;
        icons::draw(display, Point::new(ix, iy), 54, IconKind::from_wmo(code))?;

        let tx = ix + 64;
        Text::with_text_style(
            &format!("{:.1}\u{00b0}C", cur.temperature_2m),
            Point::new(tx, iy),
            big,
            top,
        )
        .draw(display)?;
        Text::with_text_style(wmo_text(code), Point::new(tx, iy + 22), mid, top).draw(display)?;

        let feels = cur
            .apparent_temperature
            .map(|a| format!("Feels {:.0}\u{00b0}", a))
            .unwrap_or_default();
        let hl = match (
            daily.temperature_2m_max.first(),
            daily.temperature_2m_min.first(),
        ) {
            (Some(h), Some(l)) => format!("H {:.0}\u{00b0} L {:.0}\u{00b0}", h, l),
            _ => String::new(),
        };
        Text::with_text_style(
            &format!("{}  {}", feels, hl),
            Point::new(tx, iy + 40),
            small,
            top,
        )
        .draw(display)?;

        // Stat grid: 7 icon+value cells laid out in 2 columns x 4 rows.
        let gy = iy + 56;
        let rowh = 16;
        let gsize = 14u32;
        let col1 = ix;
        let col2 = ix + iw / 2;

        let hum = cur
            .relative_humidity_2m
            .map(|h| format!("{:.0}%", h))
            .unwrap_or_else(|| "--".into());
        let wind = match (cur.wind_speed_10m, cur.wind_direction_10m) {
            (Some(s), Some(d)) => format!("{:.0} km/h {}", s, compass(d)),
            (Some(s), None) => format!("{:.0} km/h", s),
            _ => "--".into(),
        };
        let rain = daily
            .precipitation_probability_max
            .first()
            .and_then(|v| *v)
            .map(|v| format!("{}%", v))
            .unwrap_or_else(|| "--".into());
        let uv = daily
            .uv_index_max
            .first()
            .and_then(|v| *v)
            .map(|v| format!("{:.0}", v))
            .unwrap_or_else(|| "--".into());
        let sunrise = daily
            .sunrise
            .first()
            .map(|s| format!("{}h", hhmm(s)))
            .unwrap_or_else(|| "--".into());
        let sunset = daily
            .sunset
            .first()
            .map(|s| format!("{}h", hhmm(s)))
            .unwrap_or_else(|| "--".into());
        let sun_h = daily
            .sunshine_duration
            .first()
            .map(|s| format!("{:.1}h", s / 3600.0))
            .unwrap_or_else(|| "--".into());

        let cells: [(Glyph, &str, i32, i32); 7] = [
            (glyphs::humidity, hum.as_str(), col1, 0),
            (glyphs::wind, wind.as_str(), col2, 0),
            (glyphs::rain, rain.as_str(), col1, 1),
            (glyphs::uv, uv.as_str(), col2, 1),
            (glyphs::sunrise, sunrise.as_str(), col1, 2),
            (glyphs::sunset, sunset.as_str(), col2, 2),
            (glyphs::sunshine, sun_h.as_str(), col1, 3),
        ];
        for (glyph, value, cx, row) in cells {
            let y = gy + row * rowh;
            glyph(display, Point::new(cx, y), gsize)?;
            Text::with_text_style(value, Point::new(cx + 20, y), mid, top).draw(display)?;
        }

        Ok(())
    }
}
