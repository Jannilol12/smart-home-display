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
    Precip(Precip),
    Thunder,
}

/// A precipitation icon: its type, how heavy it is, whether it falls as an
/// intermittent shower (drawn with a peeking sun) and whether it is freezing
/// (drawn with a short "ice" bar underneath).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Precip {
    pub kind: PrecipKind,
    pub level: Intensity,
    pub shower: bool,
    pub freezing: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrecipKind {
    Drizzle,
    Rain,
    Snow,
}

/// The three WMO precipitation gradings (light / moderate / heavy). Drawn as a
/// different number of falling glyphs so the strength is readable at a glance.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Intensity {
    Light,
    Moderate,
    Heavy,
}

impl IconKind {
    pub fn from_wmo(code: u16) -> Self {
        use Intensity::{Heavy, Light, Moderate};
        use PrecipKind::{Drizzle, Rain, Snow};
        let precip = |kind, level, shower, freezing| {
            IconKind::Precip(Precip {
                kind,
                level,
                shower,
                freezing,
            })
        };
        match code {
            0 | 1 => IconKind::Clear,
            2 => IconKind::PartlyCloudy,
            3 => IconKind::Cloudy,
            45 | 48 => IconKind::Fog,
            // Drizzle: light, moderate, dense
            51 => precip(Drizzle, Light, false, false),
            53 => precip(Drizzle, Moderate, false, false),
            55 => precip(Drizzle, Heavy, false, false),
            // Freezing drizzle: light, dense
            56 => precip(Drizzle, Light, false, true),
            57 => precip(Drizzle, Heavy, false, true),
            // Rain: slight, moderate, heavy
            61 => precip(Rain, Light, false, false),
            63 => precip(Rain, Moderate, false, false),
            65 => precip(Rain, Heavy, false, false),
            // Freezing rain: light, heavy
            66 => precip(Rain, Light, false, true),
            67 => precip(Rain, Heavy, false, true),
            // Rain showers: slight, moderate, violent
            80 => precip(Rain, Light, true, false),
            81 => precip(Rain, Moderate, true, false),
            82 => precip(Rain, Heavy, true, false),
            // Snow fall: slight, moderate, heavy
            71 => precip(Snow, Light, false, false),
            73 => precip(Snow, Moderate, false, false),
            75 => precip(Snow, Heavy, false, false),
            // Snow grains
            77 => precip(Snow, Light, false, false),
            // Snow showers: slight, heavy
            85 => precip(Snow, Light, true, false),
            86 => precip(Snow, Heavy, true, false),
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
        IconKind::Precip(p) => {
            if p.shower {
                // A sun peeking from behind the cloud marks intermittent showers.
                sun(
                    display,
                    Point::new((x + s * 0.68) as i32, (y + s * 0.28) as i32),
                    (s * 0.32) as u32,
                    true,
                    true,
                )?;
                cloud(
                    display,
                    Point::new((x + s * 0.02) as i32, (y + s * 0.22) as i32),
                    (s * 0.74) as u32,
                )?;
            } else if matches!(p.kind, PrecipKind::Rain) {
                // Rain gets a solid cloud; drizzle and snow get a hollow one so
                // the lighter precipitation reads as a lighter-weight icon.
                cloud(
                    display,
                    Point::new((x + s * 0.10) as i32, (y + s * 0.06) as i32),
                    (s * 0.80) as u32,
                )?;
            } else {
                cloud_outline(
                    display,
                    Point::new((x + s * 0.10) as i32, (y + s * 0.06) as i32),
                    (s * 0.80) as u32,
                )?;
            }
            precip_glyphs(display, x, y, s, p)?;
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

/// Draw the falling-precipitation glyphs beneath a cloud. The glyph COUNT still
/// encodes intensity (light = 2, moderate = 3, heavy = 4).
fn precip_glyphs(
    display: &mut Display7in5,
    x: f32,
    y: f32,
    s: f32,
    p: Precip,
) -> Result<(), Infallible> {
    let count = match p.level {
        Intensity::Light => 2,
        Intensity::Moderate => 3,
        Intensity::Heavy => 4,
    };
    // Showers sit under a smaller, left-shifted cloud and start a little lower.
    let cx = if p.shower { x + s * 0.40 } else { x + s * 0.50 };
    let base = if p.shower { y + s * 0.70 } else { y + s * 0.62 };
    let step = s * 0.14; // horizontal spacing between glyphs (kept tight)
    let stagger = s * 0.11; // vertical offset applied to every other glyph
    let start = cx - step * (count as f32 - 1.0) * 0.5;
    for i in 0..count {
        let fx = start + step * i as f32;
        let dy = if i % 2 == 1 { stagger } else { 0.0 };
        match p.kind {
            // Drizzle: fine drops whose tips sit slightly up inside the cloud.
            PrecipKind::Drizzle => {
                waterdrop(
                    display,
                    Point::new(fx as i32, (base - s * 0.12 + dy) as i32),
                    s * 0.20,
                    s * 0.12,
                )?;
            }
            PrecipKind::Rain => match p.level {
                // Light rain: bigger water drops.
                Intensity::Light => {
                    waterdrop(
                        display,
                        Point::new(fx as i32, (base - s * 0.02 + dy) as i32),
                        s * 0.24,
                        s * 0.14,
                    )?;
                }
                // Heavier rain: slanted streaks broken by a single gap whose
                // position cycles begin -> middle -> end across the streaks.
                _ => {
                    let top = base + dy;
                    let gap_center = [0.28_f32, 0.52, 0.76][i % 3];
                    dashed_line(
                        display,
                        Point::new(fx as i32, top as i32),
                        Point::new((fx - s * 0.05) as i32, (top + s * 0.22) as i32),
                        gap_center,
                        0.13,
                    )?;
                }
            },
            // Snow: six-spoke flakes.
            PrecipKind::Snow => {
                snowflake(
                    display,
                    Point::new(fx as i32, (base + s * 0.05 + dy) as i32),
                    s * 0.11,
                )?;
            }
        }
    }
    if p.freezing {
        let ly = (y + s * 0.95) as i32;
        Line::new(
            Point::new((x + s * 0.22) as i32, ly),
            Point::new((x + s * 0.78) as i32, ly),
        )
        .draw_styled(&stroke(2), display)?;
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
    cloud_shape(display, top_left, width, 1.0, &fill())
}

/// A hollow cloud
fn cloud_outline(display: &mut Display7in5, top_left: Point, width: u32) -> Result<(), Infallible> {
    cloud_shape(display, top_left, width, 1.0, &fill())?;
    cloud_shape(
        display,
        top_left,
        width,
        0.78,
        &PrimitiveStyle::with_fill(Color::White),
    )
}

/// The cloud silhouette
fn cloud_shape(
    display: &mut Display7in5,
    top_left: Point,
    width: u32,
    scale: f32,
    style: &PrimitiveStyle<Color>,
) -> Result<(), Infallible> {
    let w = width as f32;
    let x = top_left.x as f32;
    let y = top_left.y as f32;
    let scx = x + w * 0.50;
    let scy = y + w * 0.40;
    // Scale a point toward the cloud centre.
    let sp = |px: f32, py: f32| {
        Point::new(
            (scx + (px - scx) * scale) as i32,
            (scy + (py - scy) * scale) as i32,
        )
    };
    let dia = |r: f32| (r * 2.0 * scale) as u32;
    Circle::with_center(sp(x + w * 0.18, y + w * 0.46), dia(w * 0.18))
        .draw_styled(style, display)?;
    Circle::with_center(sp(x + w * 0.37, y + w * 0.22), dia(w * 0.22))
        .draw_styled(style, display)?;
    Circle::with_center(sp(x + w * 0.67, y + w * 0.25), dia(w * 0.13))
        .draw_styled(style, display)?;
    Circle::with_center(sp(x + w * 0.80, y + w * 0.44), dia(w * 0.20))
        .draw_styled(style, display)?;
    Circle::with_center(sp(x + w * 0.48, y + w * 0.37), dia(w * 0.22))
        .draw_styled(style, display)?;
    let yb = y + w * 0.64;
    let rtop = y + w * 0.42;
    Rectangle::new(
        sp(x + w * 0.17, rtop),
        Size::new((w * 0.66 * scale) as u32, ((yb - rtop) * scale) as u32),
    )
    .draw_styled(style, display)?;
    Ok(())
}

/// A falling water drop
fn waterdrop(
    display: &mut Display7in5,
    tip: Point,
    height: f32,
    width: f32,
) -> Result<(), Infallible> {
    let rb = width * 0.5;
    let bulb_cy = tip.y as f32 + height - rb;
    Triangle::new(
        tip,
        Point::new((tip.x as f32 - rb) as i32, bulb_cy as i32),
        Point::new((tip.x as f32 + rb) as i32, bulb_cy as i32),
    )
    .draw_styled(&fill(), display)?;
    Circle::with_center(Point::new(tip.x, bulb_cy as i32), width as u32)
        .draw_styled(&fill(), display)?;
    Ok(())
}

/// Draw a line split by a single gap, so it reads as an interrupted rain
/// streak. `gap_center` and `gap_half` are fractions (0..1) of the line length;
/// the segments before and after the gap are drawn, the gap itself left blank.
fn dashed_line(
    display: &mut Display7in5,
    p0: Point,
    p1: Point,
    gap_center: f32,
    gap_half: f32,
) -> Result<(), Infallible> {
    let x0 = p0.x as f32;
    let y0 = p0.y as f32;
    let dx = p1.x as f32 - x0;
    let dy = p1.y as f32 - y0;
    let at = |t: f32| Point::new((x0 + dx * t) as i32, (y0 + dy * t) as i32);
    let g0 = gap_center - gap_half;
    let g1 = gap_center + gap_half;
    if g0 > 0.0 {
        Line::new(at(0.0), at(g0.min(1.0))).draw_styled(&stroke(2), display)?;
    }
    if g1 < 1.0 {
        Line::new(at(g1.max(0.0)), at(1.0)).draw_styled(&stroke(2), display)?;
    }
    Ok(())
}

/// A six-spoke snowflake (one vertical spoke plus two diagonals) centred on
/// `center` with the given spoke `radius`.
fn snowflake(display: &mut Display7in5, center: Point, radius: f32) -> Result<(), Infallible> {
    let cx = center.x as f32;
    let cy = center.y as f32;
    const DIRS: [(f32, f32); 3] = [(0.0, 1.0), (0.866, 0.5), (-0.866, 0.5)];
    for (dx, dy) in DIRS {
        Line::new(
            Point::new((cx - dx * radius) as i32, (cy - dy * radius) as i32),
            Point::new((cx + dx * radius) as i32, (cy + dy * radius) as i32),
        )
        .draw_styled(&stroke(2), display)?;
    }
    Ok(())
}
