use core::convert::Infallible;

use embedded_graphics::{
    mono_font::{
        iso_8859_1::{FONT_9X15, FONT_9X15_BOLD},
        MonoTextStyleBuilder,
    },
    prelude::*,
    primitives::Rectangle,
    text::{Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};
use esp_idf_svc::http::{client::EspHttpConnection, Method};
use log::error;
use serde::Deserialize;

use super::{DisplayModule, UpdateCtx};
use crate::helpers::{draw_panel, month_abbr, truncate, util, weekday};

const MAX_EVENTS: usize = 5;

/// Google Calendar OAuth access (installed-app flow), injected at construction
/// from `.secret`. An empty `refresh_token` disables the calendar block.
pub struct GcalConfig {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub calendar_id: String,
}

/// One upcoming appointment, pre-formatted for rendering.
struct CalEvent {
    when: String,
    title: String,
}

/// Bottom full-width block: the next Google Calendar appointments.
pub struct CalendarModule {
    bounds: Rectangle,
    cfg: GcalConfig,
    /// Cached OAuth access token, minted on the slow cadence and reused.
    token: Option<String>,
    events: Vec<CalEvent>,
    status: String,
}

/// Minimal projection of the Google Calendar `events.list` response.
#[derive(Deserialize)]
struct EventsResponse {
    #[serde(default)]
    items: Vec<GEvent>,
}

#[derive(Deserialize)]
struct GEvent {
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    start: GWhen,
    #[serde(default)]
    end: GWhen,
}

#[derive(Deserialize, Default)]
struct GWhen {
    /// Timed events: RFC3339 e.g. `2026-08-20T14:30:00+02:00`.
    #[serde(rename = "dateTime", default)]
    date_time: Option<String>,
    /// All-day events: `2026-08-21`.
    #[serde(default)]
    date: Option<String>,
}

impl CalendarModule {
    pub fn new(bounds: Rectangle, cfg: GcalConfig) -> Self {
        let status = if cfg.refresh_token.is_empty() {
            "Google Calendar not configured (see .secret)".to_string()
        } else {
            "Loading...".to_string()
        };
        Self {
            bounds,
            cfg,
            token: None,
            events: Vec::new(),
            status,
        }
    }

    /// Refresh a short-lived OAuth access token from the stored refresh token
    /// (installed-app / desktop flow).
    fn access_token(&self, client: &mut EspHttpConnection) -> anyhow::Result<String> {
        let body = format!(
            "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
            self.cfg.client_id, self.cfg.client_secret, self.cfg.refresh_token
        );
        let len = body.len().to_string();
        let headers = [
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Content-Length", len.as_str()),
            ("Accept", "application/json"),
        ];
        client.initiate_request(
            Method::Post,
            "https://oauth2.googleapis.com/token",
            &headers,
        )?;
        client.write_all(body.as_bytes())?;
        client.initiate_response()?;
        let resp = util::read_body(client)?;

        #[derive(Deserialize)]
        struct Token {
            #[serde(default)]
            access_token: String,
        }
        let token: Token = serde_json::from_str(&resp)
            .map_err(|e| anyhow::anyhow!("token response parse failed: {e}"))?;
        if token.access_token.is_empty() {
            anyhow::bail!("no access_token in token response");
        }
        Ok(token.access_token)
    }

    /// Fetch upcoming events for the configured calendar starting at `time_min`
    /// (RFC3339 UTC).
    fn events(
        &self,
        client: &mut EspHttpConnection,
        token: &str,
        time_min: &str,
    ) -> anyhow::Result<String> {
        let cal_id = if self.cfg.calendar_id.is_empty() {
            "primary".to_string()
        } else {
            self.cfg.calendar_id.replace('@', "%40")
        };
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{cal_id}/events?singleEvents=true&orderBy=startTime&maxResults=6&timeMin={time_min}&fields=items(summary,start,end)"
        );
        let auth = format!("Bearer {token}");
        let headers = [
            ("Authorization", auth.as_str()),
            ("Accept", "application/json"),
        ];
        client.initiate_request(Method::Get, &url, &headers)?;
        client.initiate_response()?;
        util::read_body(client)
    }

    /// Parse a Google Calendar events JSON body into renderable rows. `today`
    /// is the local `YYYY-MM-DD` date, used to decide when to show a month.
    pub fn load(&mut self, json: &str, today: &str) -> Result<(), ()> {
        let resp: EventsResponse = serde_json::from_str(json).map_err(|_| ())?;
        self.events.clear();
        for ev in resp.items.into_iter().take(MAX_EVENTS) {
            let title = ev.summary.unwrap_or_else(|| "(no title)".to_string());
            let when = if let Some(dt) = ev.start.date_time {
                let date = dt.get(0..10).unwrap_or("");
                let time = dt.get(11..16).unwrap_or("");
                format!("{} {}", fmt_day(date, today), time)
            } else if let Some(d) = ev.start.date {
                // All-day event: Google's end.date is exclusive, so the real
                // last day is end.date - 1. Show a range only if multi-day.
                match ev.end.date.as_deref().map(prev_day) {
                    Some(ref e) if e.as_str() > d.as_str() => {
                        format!("{} - {}", fmt_day(&d, today), fmt_day(e, today))
                    }
                    _ => fmt_day(&d, today),
                }
            } else {
                String::new()
            };
            self.events.push(CalEvent { when, title });
        }
        self.status = if self.events.is_empty() {
            "No upcoming events".to_string()
        } else {
            String::new()
        };
        Ok(())
    }

    /// Show a short status line instead of events (e.g. when unconfigured).
    pub fn set_status(&mut self, status: &str) {
        self.events.clear();
        self.status = status.to_string();
    }

    /// A compact fingerprint of the current calendar content, used to decide
    /// whether the panel actually needs re-rendering.
    pub fn signature(&self) -> String {
        if !self.status.is_empty() {
            return format!("!{}", self.status);
        }
        self.events
            .iter()
            .map(|e| format!("{}|{};", e.when, e.title))
            .collect()
    }
}

impl DisplayModule for CalendarModule {
    fn bounds(&self) -> Rectangle {
        self.bounds
    }

    fn update(&mut self, ctx: &mut UpdateCtx) -> bool {
        if self.cfg.refresh_token.is_empty() {
            return false; // unconfigured: keep the static status set at construction
        }
        // The OAuth token is minted on the slow cadence and reused (valid ~1h);
        // a failed events call drops it so it is re-minted on the next poll.
        if ctx.slow || self.token.is_none() {
            match self.access_token(ctx.client) {
                Ok(t) => self.token = Some(t),
                Err(e) => {
                    error!("   Calendar auth failed: {e:?}");
                    self.set_status("Calendar auth failed");
                }
            }
        }
        let prev = self.signature();
        if let Some(token) = self.token.take() {
            let time_min = util::rfc3339_utc_now();
            match self.events(ctx.client, &token, &time_min) {
                Ok(body) => {
                    self.token = Some(token);
                    if self.load(&body, &util::ymd(&ctx.now)).is_err() {
                        error!("   Failed to parse calendar events.");
                        self.set_status("Calendar parse error");
                    }
                }
                Err(e) => {
                    error!("   Calendar events request failed: {e:?}");
                    self.set_status("Calendar unavailable");
                }
            }
        }
        self.signature() != prev
    }

    fn render(&self, display: &mut Display7in5) -> Result<(), Infallible> {
        let inner = draw_panel(display, self.bounds, "CALENDAR")?;
        let ix = inner.top_left.x;
        let iy = inner.top_left.y;
        let iw = inner.size.width as i32;

        let bold = MonoTextStyleBuilder::new()
            .font(&FONT_9X15_BOLD)
            .text_color(Color::Black)
            .build();
        let mid = MonoTextStyleBuilder::new()
            .font(&FONT_9X15)
            .text_color(Color::Black)
            .build();
        let top = TextStyleBuilder::new().baseline(Baseline::Top).build();

        if self.events.is_empty() {
            Text::with_text_style(&self.status, Point::new(ix, iy + 4), mid, top).draw(display)?;
            return Ok(());
        }

        let rowh = 20;
        for (i, ev) in self.events.iter().enumerate() {
            let y = iy + i as i32 * rowh;
            Text::with_text_style(&ev.when, Point::new(ix, y), bold, top).draw(display)?;
            // Title starts right after the (variable-width) date/time column and
            // is clipped with an ellipsis to stay inside the box.
            let title_x = ix + ev.when.chars().count() as i32 * 9 + 8;
            let title_chars = ((ix + iw - title_x) / 9).max(0) as usize;
            Text::with_text_style(
                &truncate(&ev.title, title_chars),
                Point::new(title_x, y),
                mid,
                top,
            )
            .draw(display)?;
        }

        Ok(())
    }
}

/// Format a date as "Wed 24", adding a 3-letter month ("Wed 1 Sep") when it
/// falls outside `today`'s month. Day numbers drop any leading zero.
fn fmt_day(date: &str, today: &str) -> String {
    let wd = weekday(date);
    let raw = date.get(8..10).unwrap_or("");
    let trimmed = raw.trim_start_matches('0');
    let day = if trimmed.is_empty() { raw } else { trimmed };
    if date.get(0..7) == today.get(0..7) {
        format!("{} {}", wd, day)
    } else {
        let m: usize = date.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(0);
        format!("{} {} {}", wd, day, month_abbr(m))
    }
}

/// The calendar date (YYYY-MM-DD) one day before `date`.
fn prev_day(date: &str) -> String {
    let mut y: i32 = date.get(0..4).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let mut m: i32 = date.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut d: i32 = date.get(8..10).and_then(|s| s.parse().ok()).unwrap_or(1);
    d -= 1;
    if d < 1 {
        m -= 1;
        if m < 1 {
            m = 12;
            y -= 1;
        }
        d = days_in_month(y, m as usize);
    }
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn days_in_month(y: i32, m: usize) -> i32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}
