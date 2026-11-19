use core::convert::Infallible;
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_10X20, FONT_9X15},
        MonoTextStyleBuilder,
    },
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

use super::{DisplayModule, UpdateCtx};
use crate::helpers::util;

/// Top bar showing the current date, time and the device IP address (or
/// OFFLINE while Wi-Fi is down).
pub struct HeaderModule {
    bounds: Rectangle,
    date: String,
    time: String,
    /// "IP a.b.c.d" or "OFFLINE".
    net: String,
}

impl HeaderModule {
    pub fn new(bounds: Rectangle) -> Self {
        Self {
            bounds,
            date: String::from("--"),
            time: String::from("--:--"),
            net: String::new(),
        }
    }
}

impl DisplayModule for HeaderModule {
    fn bounds(&self) -> Rectangle {
        self.bounds
    }

    fn update(&mut self, ctx: &mut UpdateCtx) -> bool {
        // Until SNTP has set the clock it reads 1970: show placeholders.
        let (date, time) = if ctx.time_valid {
            util::local_datetime()
        } else {
            ("--".to_string(), "--:--".to_string())
        };
        let net = match &ctx.ip {
            Some(ip) => format!("IP {ip}"),
            None => "OFFLINE".to_string(),
        };
        let changed = time != self.time || date != self.date || net != self.net;
        self.date = date;
        self.time = time;
        self.net = net;
        changed
    }

    fn render(&self, display: &mut Display7in5) -> Result<(), Infallible> {
        let tl = self.bounds.top_left;
        let w = self.bounds.size.width as i32;
        let h = self.bounds.size.height as i32;
        let mid_y = tl.y + (h - 20) / 2;

        let big = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(Color::Black)
            .build();
        let ip_style = MonoTextStyleBuilder::new()
            .font(&FONT_9X15)
            .text_color(Color::Black)
            .build();

        let left = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Left)
            .build();
        let center = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Center)
            .build();
        let right = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Right)
            .build();

        Text::with_text_style(&self.date, Point::new(tl.x + 10, mid_y), big, left).draw(display)?;
        Text::with_text_style(&self.time, Point::new(tl.x + w / 2, mid_y), big, center)
            .draw(display)?;
        Text::with_text_style(
            &self.net,
            Point::new(tl.x + w - 10, mid_y + 2),
            ip_style,
            right,
        )
        .draw(display)?;

        // Divider under the header.
        let y = tl.y + h - 1;
        Line::new(Point::new(tl.x, y), Point::new(tl.x + w - 1, y))
            .draw_styled(&PrimitiveStyle::with_stroke(Color::Black, 2), display)?;

        Ok(())
    }
}
