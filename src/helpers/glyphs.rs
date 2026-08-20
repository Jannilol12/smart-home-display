//! Small monochrome stat glyphs (~14-16px) used to label the current-weather
//! metrics with icons instead of words. Each function fills an `s` x `s` box
//! anchored at `top_left`, drawn in the natural "black on white" convention
//! (the controller inverts the framebuffer before flushing to the panel).

use core::convert::Infallible;
use embedded_graphics::{
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, StyledDrawable, Triangle},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

fn fill() -> PrimitiveStyle<Color> {
    PrimitiveStyle::with_fill(Color::Black)
}

fn stroke(width: u32) -> PrimitiveStyle<Color> {
    PrimitiveStyle::with_stroke(Color::Black, width)
}

fn pt(x: f32, y: f32) -> Point {
    Point::new(x as i32, y as i32)
}

const RAY_DIRS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (0.707, 0.707),
    (0.0, 1.0),
    (-0.707, 0.707),
    (-1.0, 0.0),
    (-0.707, -0.707),
    (0.0, -1.0),
    (0.707, -0.707),
];

/// Humidity: a filled tear-drop (cone on top of a disc).
pub fn humidity(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    let s = size as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;
    let cx = x + s * 0.5;
    let cyc = y + s * 0.62;
    let r = s * 0.30;
    Circle::with_center(pt(cx, cyc), (s * 0.60) as u32).draw_styled(&fill(), display)?;
    Triangle::new(pt(cx, y + s * 0.06), pt(cx - r, cyc), pt(cx + r, cyc))
        .draw_styled(&fill(), display)?;
    Ok(())
}

/// Wind: three horizontal gust lines, the top and bottom curling at the end.
pub fn wind(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    let s = size as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;
    let x0 = x + s * 0.10;

    Line::new(pt(x0, y + s * 0.32), pt(x + s * 0.60, y + s * 0.32))
        .draw_styled(&stroke(2), display)?;
    Line::new(
        pt(x + s * 0.60, y + s * 0.32),
        pt(x + s * 0.72, y + s * 0.22),
    )
    .draw_styled(&stroke(2), display)?;
    Line::new(
        pt(x + s * 0.72, y + s * 0.22),
        pt(x + s * 0.58, y + s * 0.14),
    )
    .draw_styled(&stroke(2), display)?;

    Line::new(pt(x0, y + s * 0.52), pt(x + s * 0.74, y + s * 0.52))
        .draw_styled(&stroke(2), display)?;

    Line::new(pt(x0, y + s * 0.72), pt(x + s * 0.55, y + s * 0.72))
        .draw_styled(&stroke(2), display)?;
    Line::new(
        pt(x + s * 0.55, y + s * 0.72),
        pt(x + s * 0.67, y + s * 0.82),
    )
    .draw_styled(&stroke(2), display)?;
    Line::new(
        pt(x + s * 0.67, y + s * 0.82),
        pt(x + s * 0.53, y + s * 0.90),
    )
    .draw_styled(&stroke(2), display)?;
    Ok(())
}

/// Rain: a small filled cloud with two falling drops.
pub fn rain(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    let s = size as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;

    Circle::with_center(pt(x + s * 0.40, y + s * 0.36), (s * 0.36) as u32)
        .draw_styled(&fill(), display)?;
    Circle::with_center(pt(x + s * 0.62, y + s * 0.34), (s * 0.30) as u32)
        .draw_styled(&fill(), display)?;
    Circle::with_center(pt(x + s * 0.28, y + s * 0.42), (s * 0.26) as u32)
        .draw_styled(&fill(), display)?;
    Rectangle::new(
        pt(x + s * 0.20, y + s * 0.36),
        Size::new((s * 0.48) as u32, (s * 0.16) as u32),
    )
    .draw_styled(&fill(), display)?;

    Line::new(
        pt(x + s * 0.36, y + s * 0.60),
        pt(x + s * 0.30, y + s * 0.82),
    )
    .draw_styled(&stroke(2), display)?;
    Line::new(
        pt(x + s * 0.58, y + s * 0.60),
        pt(x + s * 0.52, y + s * 0.82),
    )
    .draw_styled(&stroke(2), display)?;
    Ok(())
}

/// Sunrise: a sun rising over the horizon with an up arrow.
pub fn sunrise(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    sun_horizon(display, top_left, size, true)
}

/// Sunset: a sun dropping below the horizon with a down arrow.
pub fn sunset(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    sun_horizon(display, top_left, size, false)
}

fn sun_horizon(
    display: &mut Display7in5,
    top_left: Point,
    size: u32,
    up: bool,
) -> Result<(), Infallible> {
    let s = size as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;

    // Ground line.
    let gy = y + s * 0.74;
    Line::new(pt(x + s * 0.04, gy), pt(x + s * 0.60, gy)).draw_styled(&stroke(2), display)?;

    // Sun disc + short upward rays.
    let scx = x + s * 0.30;
    let scy = y + s * 0.56;
    Circle::with_center(pt(scx, scy), (s * 0.30) as u32).draw_styled(&fill(), display)?;
    for (dx, dy) in [(-0.5, -0.87), (0.0, -1.0), (0.5, -0.87)] {
        let r_in = s * 0.22;
        let r_out = s * 0.36;
        Line::new(
            pt(scx + dx * r_in, scy + dy * r_in),
            pt(scx + dx * r_out, scy + dy * r_out),
        )
        .draw_styled(&stroke(2), display)?;
    }

    // Direction arrow on the right.
    let ax = x + s * 0.82;
    if up {
        Line::new(pt(ax, y + s * 0.66), pt(ax, y + s * 0.40)).draw_styled(&stroke(2), display)?;
        Triangle::new(
            pt(ax, y + s * 0.28),
            pt(ax - s * 0.13, y + s * 0.44),
            pt(ax + s * 0.13, y + s * 0.44),
        )
        .draw_styled(&fill(), display)?;
    } else {
        Line::new(pt(ax, y + s * 0.40), pt(ax, y + s * 0.66)).draw_styled(&stroke(2), display)?;
        Triangle::new(
            pt(ax, y + s * 0.78),
            pt(ax - s * 0.13, y + s * 0.62),
            pt(ax + s * 0.13, y + s * 0.62),
        )
        .draw_styled(&fill(), display)?;
    }
    Ok(())
}

/// UV index: a hollow sun (ring + rays) suggesting radiation exposure.
pub fn uv(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    sun_glyph(display, top_left, size, false)
}

/// Sunshine hours: a solid sun (filled disc + rays).
pub fn sunshine(display: &mut Display7in5, top_left: Point, size: u32) -> Result<(), Infallible> {
    sun_glyph(display, top_left, size, true)
}

/// Precipitation amount (mm): a droplet resting on a small puddle line.
pub fn millimeters(
    display: &mut Display7in5,
    top_left: Point,
    size: u32,
) -> Result<(), Infallible> {
    let s = size as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;
    let cx = x + s * 0.5;
    let cyc = y + s * 0.50;
    let r = s * 0.26;
    Circle::with_center(pt(cx, cyc), (s * 0.52) as u32).draw_styled(&fill(), display)?;
    Triangle::new(pt(cx, y + s * 0.02), pt(cx - r, cyc), pt(cx + r, cyc))
        .draw_styled(&fill(), display)?;
    Line::new(
        pt(x + s * 0.14, y + s * 0.92),
        pt(x + s * 0.86, y + s * 0.92),
    )
    .draw_styled(&stroke(2), display)?;
    Ok(())
}

fn sun_glyph(
    display: &mut Display7in5,
    top_left: Point,
    size: u32,
    filled: bool,
) -> Result<(), Infallible> {
    let s = size as f32;
    let cx = top_left.x as f32 + s * 0.5;
    let cy = top_left.y as f32 + s * 0.5;
    let dia = s * 0.44;
    if filled {
        Circle::with_center(pt(cx, cy), dia as u32).draw_styled(&fill(), display)?;
    } else {
        Circle::with_center(pt(cx, cy), dia as u32).draw_styled(&stroke(2), display)?;
    }
    let r_in = dia * 0.5 + s * 0.06;
    let r_out = r_in + s * 0.16;
    for (dx, dy) in RAY_DIRS {
        Line::new(
            pt(cx + dx * r_in, cy + dy * r_in),
            pt(cx + dx * r_out, cy + dy * r_out),
        )
        .draw_styled(&stroke(2), display)?;
    }
    Ok(())
}
