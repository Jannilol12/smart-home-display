use core::convert::Infallible;
use embedded_graphics::{
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, StyledDrawable, Triangle},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Thunder,
}

impl IconKind {
    pub fn from_wmo(code: u16) -> Self {
        match code {
            0 | 1 => IconKind::Clear,
            2 => IconKind::PartlyCloudy,
            3 => IconKind::Cloudy,
            45 | 48 => IconKind::Fog,
            51..=57 | 61..=67 | 80..=82 => IconKind::Rain,
            71..=77 | 85 | 86 => IconKind::Snow,
            95..=99 => IconKind::Thunder,
            _ => IconKind::Cloudy,
        }
    }
}

fn fill() -> PrimitiveStyle<Color> {
    PrimitiveStyle::with_fill(Color::Black)
}

fn stroke(width: u32) -> PrimitiveStyle<Color> {
    PrimitiveStyle::with_stroke(Color::Black, width)
}

/// Draw a weather icon that fills a `size` x `size` box anchored at `top_left`.
pub fn draw(
    display: &mut Display7in5,
    top_left: Point,
    size: u32,
    kind: IconKind,
) -> Result<(), Infallible> {
    let s = size as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;
    let cx = (x + s * 0.5) as i32;
    let cy = (y + s * 0.5) as i32;

    match kind {
        IconKind::Clear => {
            sun(display, Point::new(cx, cy), (s * 0.5) as u32, false, true)?;
        }
        IconKind::PartlyCloudy => {
            sun(
                display,
                Point::new((x + s * 0.66) as i32, (y + s * 0.34) as i32),
                (s * 0.34) as u32,
                true,
                true,
            )?;
            cloud(
                display,
                Point::new((x + s * 0.04) as i32, (y + s * 0.34) as i32),
                (s * 0.74) as u32,
            )?;
        }
        IconKind::Cloudy => {
            cloud(
                display,
                Point::new((x + s * 0.10) as i32, (y + s * 0.22) as i32),
                (s * 0.80) as u32,
            )?;
        }
        IconKind::Fog => {
            cloud(
                display,
                Point::new((x + s * 0.10) as i32, (y + s * 0.10) as i32),
                (s * 0.80) as u32,
            )?;
            for i in 0..3 {
                let ly = (y + s * 0.72 + i as f32 * s * 0.11) as i32;
                Line::new(
                    Point::new((x + s * 0.14) as i32, ly),
                    Point::new((x + s * 0.86) as i32, ly),
                )
                .draw_styled(&stroke(2), display)?;
            }
        }
        IconKind::Rain => {
            cloud(
                display,
                Point::new((x + s * 0.10) as i32, (y + s * 0.06) as i32),
                (s * 0.80) as u32,
            )?;
            for i in 0..3 {
                let dx = (x + s * 0.30 + i as f32 * s * 0.20) as i32;
                Line::new(
                    Point::new(dx, (y + s * 0.70) as i32),
                    Point::new(dx - (s * 0.07) as i32, (y + s * 0.92) as i32),
                )
                .draw_styled(&stroke(2), display)?;
            }
        }
        IconKind::Snow => {
            cloud(
                display,
                Point::new((x + s * 0.10) as i32, (y + s * 0.06) as i32),
                (s * 0.80) as u32,
            )?;
            for i in 0..3 {
                let dx = (x + s * 0.28 + i as f32 * s * 0.22) as i32;
                Circle::with_center(Point::new(dx, (y + s * 0.82) as i32), (s * 0.08) as u32)
                    .draw_styled(&fill(), display)?;
            }
        }
        IconKind::Thunder => {
            cloud(
                display,
                Point::new((x + s * 0.10) as i32, (y + s * 0.06) as i32),
                (s * 0.80) as u32,
            )?;
            let bx = x + s * 0.46;
            let by = y + s * 0.58;
            Triangle::new(
                Point::new(bx as i32, by as i32),
                Point::new((bx + s * 0.16) as i32, by as i32),
                Point::new((bx - s * 0.02) as i32, (by + s * 0.34) as i32),
            )
            .draw_styled(&fill(), display)?;
        }
    }
    Ok(())
}

fn sun(
    display: &mut Display7in5,
    center: Point,
    diameter: u32,
    outline: bool,
    rays: bool,
) -> Result<(), Infallible> {
    let d = diameter.max(4);
    let r = d as i32 / 2;
    if outline {
        Circle::with_center(center, d).draw_styled(&stroke(2), display)?;
    } else {
        Circle::with_center(center, d).draw_styled(&fill(), display)?;
    }
    if rays {
        let inner = r + 3;
        let outer = inner + (d as i32 / 4).max(4);
        const DIRS: [(f32, f32); 8] = [
            (1.0, 0.0),
            (0.707, 0.707),
            (0.0, 1.0),
            (-0.707, 0.707),
            (-1.0, 0.0),
            (-0.707, -0.707),
            (0.0, -1.0),
            (0.707, -0.707),
        ];
        for (dx, dy) in DIRS {
            let p1 = Point::new(
                center.x + (dx * inner as f32) as i32,
                center.y + (dy * inner as f32) as i32,
            );
            let p2 = Point::new(
                center.x + (dx * outer as f32) as i32,
                center.y + (dy * outer as f32) as i32,
            );
            Line::new(p1, p2).draw_styled(&stroke(2), display)?;
        }
    }
    Ok(())
}

fn cloud(display: &mut Display7in5, top_left: Point, width: u32) -> Result<(), Infallible> {
    let w = width as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;
    Circle::with_center(
        Point::new((x + w * 0.34) as i32, (y + w * 0.42) as i32),
        (w * 0.42) as u32,
    )
    .draw_styled(&fill(), display)?;
    Circle::with_center(
        Point::new((x + w * 0.64) as i32, (y + w * 0.40) as i32),
        (w * 0.36) as u32,
    )
    .draw_styled(&fill(), display)?;
    Circle::with_center(
        Point::new((x + w * 0.22) as i32, (y + w * 0.54) as i32),
        (w * 0.30) as u32,
    )
    .draw_styled(&fill(), display)?;
    Circle::with_center(
        Point::new((x + w * 0.78) as i32, (y + w * 0.56) as i32),
        (w * 0.28) as u32,
    )
    .draw_styled(&fill(), display)?;
    Rectangle::new(
        Point::new((x + w * 0.10) as i32, (y + w * 0.44) as i32),
        Size::new((w * 0.74) as u32, (w * 0.22) as u32),
    )
    .draw_styled(&fill(), display)?;
    Ok(())
}
