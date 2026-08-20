use crate::modules::DisplayModule;
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_10X20, FONT_6X10, FONT_9X15_BOLD},
        MonoTextStyleBuilder,
    },
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::color::Color;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Deserialize, Debug, Clone, Default)]
pub struct HourlyForecast {
    #[allow(dead_code)]
    pub time: Vec<String>,
    pub temperature_2m: Vec<f32>,
    pub rain: Vec<f32>,
    pub precipitation_probability: Vec<Option<u8>>,
    pub weather_code: Vec<u16>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct OpenMeteoResponse {
    pub hourly: HourlyForecast,
}

pub struct WeatherModule {
    data: Option<OpenMeteoResponse>,
    last_update: Option<Instant>,
    refresh_interval: Duration,
}

impl WeatherModule {
    pub fn new(refresh_interval: Duration) -> Self {
        Self {
            data: None,
            last_update: None,
            refresh_interval,
        }
    }

    pub fn load_json(&mut self, json_str: &str) -> Result<(), ()> {
        if let Ok(parsed) = serde_json::from_str::<OpenMeteoResponse>(json_str) {
            self.data = Some(parsed);
            self.last_update = Some(Instant::now());
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn load_demo_data(&mut self) {
        let mut hourly = HourlyForecast::default();
        hourly.temperature_2m = vec![
            14.2, 13.8, 13.1, 12.9, 13.0, 13.5, 14.8, 16.5, 18.2, 20.1, 21.8, 22.5, 23.0, 22.8,
            22.0, 21.1, 19.8, 18.2, 17.0, 16.1, 15.5, 15.0, 14.6, 14.0,
        ];
        hourly.precipitation_probability = vec![
            Some(0),
            Some(0),
            Some(5),
            Some(10),
            Some(20),
            Some(45),
            Some(60),
            Some(80),
            Some(70),
            Some(30),
            Some(10),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(5),
            Some(10),
            Some(10),
            Some(5),
            Some(0),
            Some(0),
        ];
        hourly.weather_code = vec![3; 24]; // Partly cloudy
        hourly.rain = vec![0.0; 24];

        self.data = Some(OpenMeteoResponse { hourly });
        self.last_update = Some(Instant::now());
    }

    fn decode_wmo(code: u16) -> &'static str {
        match code {
            0 => "Clear Sky",
            1..=3 => "Partly Cloudy",
            45 | 48 => "Fog / Mist",
            51..=55 => "Drizzle",
            61..=65 => "Rain",
            71..=75 => "Snow Fall",
            80..=82 => "Rain Showers",
            95..=99 => "Thunderstorm",
            _ => "Unknown",
        }
    }
}

impl<D> DisplayModule<D> for WeatherModule
where
    D: DrawTarget<Color = Color>,
{
    fn name(&self) -> &'static str {
        "Open-Meteo Weather"
    }

    fn refresh_interval(&self) -> Duration {
        self.refresh_interval
    }

    fn last_updated(&self) -> Option<Instant> {
        self.last_update
    }

    fn update(&mut self) -> Result<(), ()> {
        if self.data.is_none() {
            self.load_demo_data();
        }
        Ok(())
    }

    fn render(&self, display: &mut D) -> Result<(), D::Error> {
        let text_top = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let title_style = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(Color::Black)
            .build();
        let bold_style = MonoTextStyleBuilder::new()
            .font(&FONT_9X15_BOLD)
            .text_color(Color::Black)
            .build();
        let small_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(Color::Black)
            .build();
        let stroke_1px = PrimitiveStyle::with_stroke(Color::Black, 1);
        let stroke_2px = PrimitiveStyle::with_stroke(Color::Black, 2);

        Text::with_text_style(
            "WEATHER FORECAST • ERLANGEN (49.59°N, 11.01°E)",
            Point::new(20, 15),
            title_style,
            text_top,
        )
        .draw(display)?;
        Text::with_text_style(
            "Source: Open-Meteo (DWD ICON-EU)",
            Point::new(20, 38),
            small_style,
            text_top,
        )
        .draw(display)?;
        Line::new(Point::new(20, 58), Point::new(780, 58)).draw_styled(&stroke_1px, display)?;

        let Some(data) = &self.data else {
            Text::with_text_style(
                "Loading forecast data...",
                Point::new(20, 100),
                bold_style,
                text_top,
            )
            .draw(display)?;
            return Ok(());
        };

        let current_temp = data.hourly.temperature_2m.first().copied().unwrap_or(0.0);
        let current_wmo = data.hourly.weather_code.first().copied().unwrap_or(0);
        let current_prob = data
            .hourly
            .precipitation_probability
            .first()
            .and_then(|p| *p)
            .unwrap_or(0);

        Rectangle::new(Point::new(20, 75), Size::new(360, 140))
            .draw_styled(&stroke_1px, display)?;
        Text::with_text_style(
            "CURRENT CONDITIONS",
            Point::new(35, 85),
            small_style,
            text_top,
        )
        .draw(display)?;
        Text::with_text_style(
            &format!("{:.1} °C", current_temp),
            Point::new(35, 105),
            title_style,
            text_top,
        )
        .draw(display)?;
        Text::with_text_style(
            &format!("Sky: {}", Self::decode_wmo(current_wmo)),
            Point::new(35, 135),
            bold_style,
            text_top,
        )
        .draw(display)?;
        Text::with_text_style(
            &format!("Precipitation Probability: {}%", current_prob),
            Point::new(35, 160),
            small_style,
            text_top,
        )
        .draw(display)?;

        Rectangle::new(Point::new(400, 75), Size::new(380, 140))
            .draw_styled(&stroke_1px, display)?;
        Text::with_text_style(
            "24-HOUR HIGHLIGHTS",
            Point::new(415, 85),
            small_style,
            text_top,
        )
        .draw(display)?;

        let points = &data.hourly.temperature_2m;
        let min_t = points.iter().take(24).fold(f32::INFINITY, |a, &b| a.min(b));
        let max_t = points
            .iter()
            .take(24)
            .fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        Text::with_text_style(
            &format!("Min: {:.1} °C    Max: {:.1} °C", min_t, max_t),
            Point::new(415, 115),
            bold_style,
            text_top,
        )
        .draw(display)?;
        let max_prob = data
            .hourly
            .precipitation_probability
            .iter()
            .take(24)
            .filter_map(|&p| p)
            .max()
            .unwrap_or(0);
        Text::with_text_style(
            &format!("Peak Rain Probability: {}%", max_prob),
            Point::new(415, 145),
            small_style,
            text_top,
        )
        .draw(display)?;

        Text::with_text_style(
            "24-HOUR TEMPERATURE TREND (°C)",
            Point::new(20, 235),
            bold_style,
            text_top,
        )
        .draw(display)?;

        let (graph_x, graph_y, graph_w, graph_h) = (70, 265, 700, 160);
        Line::new(
            Point::new(graph_x, graph_y),
            Point::new(graph_x, graph_y + graph_h),
        )
        .draw_styled(&stroke_1px, display)?;
        Line::new(
            Point::new(graph_x, graph_y + graph_h),
            Point::new(graph_x + graph_w, graph_y + graph_h),
        )
        .draw_styled(&stroke_1px, display)?;

        Text::with_text_style(
            &format!("{:.0}°", max_t),
            Point::new(25, graph_y),
            small_style,
            text_top,
        )
        .draw(display)?;
        Text::with_text_style(
            &format!("{:.0}°", min_t),
            Point::new(25, graph_y + graph_h - 10),
            small_style,
            text_top,
        )
        .draw(display)?;

        let temps_24 = points.iter().take(24).copied().collect::<Vec<f32>>();
        if temps_24.len() >= 2 {
            let t_range = (max_t - min_t).max(1.0);
            let step_x = graph_w as f32 / (temps_24.len() - 1) as f32;

            for i in 0..(temps_24.len() - 1) {
                let x1 = graph_x + (i as f32 * step_x) as i32;
                let y1 = graph_y + graph_h
                    - (((temps_24[i] - min_t) / t_range) * (graph_h - 20) as f32) as i32
                    - 10;
                let x2 = graph_x + ((i + 1) as f32 * step_x) as i32;
                let y2 = graph_y + graph_h
                    - (((temps_24[i + 1] - min_t) / t_range) * (graph_h - 20) as f32) as i32
                    - 10;

                Line::new(Point::new(x1, y1), Point::new(x2, y2))
                    .draw_styled(&stroke_2px, display)?;
                Line::new(Point::new(x1, y1 + 1), Point::new(x2, y2 + 1))
                    .draw_styled(&stroke_1px, display)?;

                if i % 6 == 0 {
                    Line::new(
                        Point::new(x1, graph_y + graph_h),
                        Point::new(x1, graph_y + graph_h + 5),
                    )
                    .draw_styled(&stroke_1px, display)?;
                    Text::with_text_style(
                        &format!("+{}h", i),
                        Point::new(x1 - 10, graph_y + graph_h + 8),
                        small_style,
                        text_top,
                    )
                    .draw(display)?;
                }
            }
        }

        Text::with_text_style(
            "Auto-refreshed via ESP32 Rust • 7.5\" EPD",
            Point::new(20, 460),
            small_style,
            text_top,
        )
        .draw(display)?;
        Ok(())
    }
}
