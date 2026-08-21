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
use esp_idf_svc::http::client::EspHttpConnection;
use log::error;

use crate::helpers::util::EspTm;
use crate::helpers::weather_data::WeatherData;

/// Everything a module needs to refresh itself during one loop iteration: the
/// shared HTTP client, the loop tick, whether this is a "slow" (30-minute)
/// tick, the current/next-hour local time, and a one-request-per-tick slot for
/// the shared weather forecast (see [`pull_weather`]).
pub struct UpdateCtx<'a> {
    pub client: &'a mut EspHttpConnection,
    pub slow: bool,
    pub now: EspTm,
    pub next_hour: EspTm,
    /// The Open-Meteo forecast, fetched at most once per tick and shared by the
    /// three weather modules via `Rc`.
    pub weather: Option<Rc<WeatherData>>,
    /// Guards against re-attempting a failed weather fetch within the same tick.
    pub weather_tried: bool,
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

/// Fetch (once per tick) and hand out the shared weather forecast. The first
/// weather module to call this on a slow tick performs the single Open-Meteo
/// request; the others reuse the cached `Rc`. Returns `None` on fast ticks or
/// when the fetch failed.
pub fn pull_weather(ctx: &mut UpdateCtx, url: &str) -> Option<Rc<WeatherData>> {
    if !ctx.slow {
        return None;
    }
    if ctx.weather.is_none() && !ctx.weather_tried {
        ctx.weather_tried = true;
        match WeatherData::fetch(ctx.client, url) {
            Ok(wd) => ctx.weather = Some(Rc::new(wd)),
            Err(e) => error!("   Weather request failed: {e:?}"),
        }
    }
    ctx.weather.clone()
}
