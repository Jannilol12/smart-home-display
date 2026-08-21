//! Shared drawing + string/date formatting helpers used across the dashboard
//! modules. Everything here draws in the natural "black on white" convention
//! (the panel uses natural polarity, so the framebuffer is flushed as-is).

use core::convert::Infallible;
use embedded_graphics::{
    mono_font::{iso_8859_1::FONT_6X10, MonoTextStyleBuilder},
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

/// Draw a titled panel (1px border + title bar) inside `bounds` and return the
/// inner content rectangle that a module may draw into.
pub fn draw_panel(
    display: &mut Display7in5,
    bounds: Rectangle,
    title: &str,
) -> Result<Rectangle, Infallible> {
    let border = PrimitiveStyle::with_stroke(Color::Black, 1);
    bounds.draw_styled(&border, display)?;

    let tl = bounds.top_left;
    let w = bounds.size.width as i32;

    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(Color::Black)
        .build();
    Text::with_text_style(
        title,
        tl + Point::new(7, 5),
        title_style,
        TextStyleBuilder::new().baseline(Baseline::Top).build(),
    )
    .draw(display)?;

    let sep_y = tl.y + 18;
    Line::new(Point::new(tl.x, sep_y), Point::new(tl.x + w - 1, sep_y))
        .draw_styled(&border, display)?;

    Ok(Rectangle::new(
        tl + Point::new(8, 23),
        Size::new(bounds.size.width - 16, bounds.size.height - 30),
    ))
}

/// Truncate a string to `max` chars, adding a `...` ellipsis if cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
        out.push_str("...");
        out
    }
}

/// Extract `HH:MM` from an ISO-8601 timestamp like `2026-08-20T14:35:00+02:00`.
pub fn hhmm(iso: &str) -> &str {
    iso.get(11..16).unwrap_or(iso)
}

/// Map a wind bearing in degrees to an 8-point compass label.
pub fn compass(deg: f32) -> &'static str {
    const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = (((deg / 45.0).round() as i32) & 7) as usize;
    DIRS[idx]
}

/// Three-letter month abbreviation for a 1-based month number.
pub fn month_abbr(m: usize) -> &'static str {
    const MO: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MO.get(m.wrapping_sub(1)).copied().unwrap_or("")
}

/// Weekday abbreviation for a `YYYY-MM-DD` date using Sakamoto's algorithm.
pub fn weekday(date: &str) -> &'static str {
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let y: i32 = date.get(0..4).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let m: usize = date.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    let d: i32 = date.get(8..10).and_then(|s| s.parse().ok()).unwrap_or(1);
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let yy = if m < 3 { y - 1 } else { y };
    let idx = (yy + yy / 4 - yy / 100 + yy / 400 + T[m - 1] + d).rem_euclid(7) as usize;
    WD[idx]
}
