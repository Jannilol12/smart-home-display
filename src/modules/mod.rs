pub mod calendar;
pub mod current_weather;
pub mod daily_forecast;
pub mod departures;
pub mod forecast_graph;
pub mod header;

use core::convert::Infallible;
use std::rc::Rc;

use embedded_graphics::primitives::Rectangle;
use epd_waveshare::epd7in5_v2::Display7in5;
use log::error;

use crate::helpers::util::{self, EspTm, Schedule};
use crate::helpers::weather_data::WeatherData;

/// Everything a module needs to refresh itself during one loop iteration: the
/// connectivity state, whether this is a "slow" (30-minute) tick, the current
/// local time, and a one-request-per-tick slot for the shared weather forecast
/// (see [`pull_weather`]).
pub struct UpdateCtx {
    /// Device IP while Wi-Fi is up; `None` = offline. Modules skip their
    /// requests while offline and keep showing their last data.
    pub ip: Option<String>,
    /// SNTP has set the clock (requests and labels derived from a 1970 clock
    /// would be bogus).
    pub time_valid: bool,
    pub slow: bool,
    pub now: EspTm,
    /// The Open-Meteo forecast, fetched at most once per tick and shared by the
    /// three weather modules via `Rc`.
    pub weather: Option<Rc<WeatherData>>,
    /// Guards against re-attempting a failed weather fetch within the same tick.
    pub weather_tried: bool,
}

impl UpdateCtx {
    pub fn new(ip: Option<String>, slow: bool) -> Self {
        Self {
            ip,
            time_valid: util::clock_is_set(),
            slow,
            now: util::local_tm(util::unix_now()),
            weather: None,
            weather_tried: false,
        }
    }

    pub fn online(&self) -> bool {
        self.ip.is_some()
    }
}

/// A single self-contained block on the dashboard. Each module knows its own
/// bounds and draws itself into the shared frame buffer. The controller simply
/// composites every module and flushes the frame once.
pub trait DisplayModule {
    fn render(&self, display: &mut Display7in5) -> Result<(), Infallible>;

    /// The module's rectangle on the panel. The controller uses this to push a
    /// partial refresh of just this block (byte-aligned to the panel's 8-pixel
    /// column granularity) when only this module changed.
    fn bounds(&self) -> Rectangle;

    /// Fetch fresh data and fold it into the module's internal state. Returns
    /// `true` when the visible content changed and the panel should be
    /// re-rendered. The default is a no-op for static modules.
    fn update(&mut self, _ctx: &mut UpdateCtx) -> bool {
        false
    }
}

/// Fetch (once per tick) and hand out the shared weather forecast. Each weather
/// module passes its own [`Schedule`]: a refresh is requested every slow tick
/// and, if it fails (or is skipped while offline), retried with back-off on the
/// following ticks instead of waiting for the next 30-minute slot. The first
/// due module performs the single Open-Meteo request; the others reuse the
/// cached `Rc`, so all three schedules stay in lockstep. Returns `None` when
/// nothing new was fetched.
pub fn pull_weather(
    ctx: &mut UpdateCtx,
    url: &str,
    schedule: &mut Schedule,
) -> Option<Rc<WeatherData>> {
    if ctx.slow {
        schedule.request();
    }
    if !ctx.online() || !schedule.due() {
        return None;
    }
    if ctx.weather.is_none() && !ctx.weather_tried {
        ctx.weather_tried = true;
        match WeatherData::fetch(url) {
            Ok(wd) => ctx.weather = Some(Rc::new(wd)),
            Err(e) => error!("   Weather request failed: {e:#}"),
        }
    }
    match ctx.weather.clone() {
        Some(wd) => {
            schedule.succeeded();
            Some(wd)
        }
        None => {
            schedule.failed();
            None
        }
    }
}
