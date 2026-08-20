//! Non-`DisplayModule` support code: shared drawing/formatting helpers, weather
//! icons and stat glyphs, the weather data model, and HTTP + local-time
//! utilities. The dashboard modules in [`crate::modules`] build on these.

pub mod draw;
pub mod glyphs;
pub mod icons;
pub mod util;
pub mod weather_data;

pub use draw::{compass, draw_panel, hhmm, month_abbr, truncate, weekday};
