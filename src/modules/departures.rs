use core::convert::Infallible;

use embedded_graphics::{
    mono_font::{
        iso_8859_1::{FONT_9X15, FONT_9X15_BOLD},
        MonoTextStyleBuilder,
    },
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, RoundedRectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use epd_waveshare::{color::Color, epd7in5_v2::Display7in5};
use esp_idf_svc::http::Method;
use log::error;
use serde::Deserialize;

use super::{DisplayModule, UpdateCtx};
use crate::helpers::util::{self, Schedule};
use crate::helpers::{draw_panel, truncate};

/// Rows shown on the board.
const MAX_ROWS: usize = 6;
/// Departures requested per poll. The spare rows keep the board filled while
/// departed trains are dropped between polls or during an outage.
const FETCH_ROWS: usize = 10;
/// Board time window. The API's default is only 30 minutes and it doesn't
/// support `entryCount` yet; a wide window capped by
/// `maxEntriesPerTransportType` amounts to "the next FETCH_ROWS departures",
/// also in the evening when trains are sparse.
const WINDOW_SECS: i64 = 3 * 3600;

/// DB OpenBoards API access (DB API Marketplace), injected at construction
/// from `.secret`.
pub struct DbConfig {
    /// API base URL (without the `/departures` endpoint).
    pub base: String,
    /// EVA number of the station.
    pub eva: String,
    pub client_id: String,
    pub api_key: String,
}

// ---- DB OpenBoards API (JSON) ----

/// Minimal projection of the OpenBoards `/departures` response. Everything
/// else in the (large) reply is skipped while it is streamed in.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Board {
    departures: Vec<Departure>,
    /// Shared stop-place table the `evaNumber` references point into.
    stop_places: Vec<StopPlace>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Departure {
    /// Best known departure time (real-time forecast when available).
    time: String,
    /// Scheduled departure time.
    time_schedule: String,
    cancelled: bool,
    journey_info: JourneyInfo,
    transport: Transport,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct JourneyInfo {
    destination: Option<StopRef>,
    /// Set when the train terminates short of its regular destination.
    differing_destination: Option<StopRef>,
    journey_cancelled: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StopRef {
    eva_number: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Transport {
    /// e.g. "S 1"; the badge fallback when there is no line name.
    journey_description: String,
    line: Option<LineInfo>,
    direction: Option<Direction>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LineInfo {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Direction {
    texts: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct StopPlace {
    eva_number: String,
    localized_names: Vec<LocalizedName>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LocalizedName {
    name: String,
}

/// A flattened departure ready for rendering, DB-board style.
struct Row {
    line: String,
    dest: String,
    planned: String,
    actual: String,
    delayed: bool,
    cancelled: bool,
    /// Best known departure as Unix time, used to drop departed trains.
    leaves: Option<i64>,
}

impl Row {
    fn new(dep: &Departure, stops: &[StopPlace]) -> Self {
        let info = &dep.journey_info;
        let stop_name = |stop: &StopRef| {
            stops
                .iter()
                .find(|s| s.eva_number == stop.eva_number)
                .and_then(|s| s.localized_names.first())
                .map(|n| n.name.clone())
        };
        let dest = info
            .differing_destination
            .as_ref()
            .or(info.destination.as_ref())
            .and_then(stop_name)
            .or_else(|| dep.transport.direction.as_ref()?.texts.first().cloned())
            .unwrap_or_else(|| "-".into());

        let line: String = dep
            .transport
            .line
            .as_ref()
            .map(|l| l.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(&dep.transport.journey_description)
            .split_whitespace()
            .collect();

        let cancelled = dep.cancelled || info.journey_cancelled;
        let (planned, _) = clock(&dep.time_schedule);
        let (actual, leaves) = clock(&dep.time);
        Self {
            line: if line.is_empty() { "?".into() } else { line },
            dest,
            delayed: !cancelled && actual != planned,
            cancelled,
            planned,
            actual,
            leaves,
        }
    }
}

/// Local `HH:MM` and Unix time of an ISO-8601 timestamp; falls back to the
/// timestamp's own clock digits if it can't be parsed.
fn clock(iso: &str) -> (String, Option<i64>) {
    match util::parse_iso8601(iso) {
        Some(t) => {
            let tm = util::local_tm(t);
            (format!("{:02}:{:02}", tm.tm_hour, tm.tm_min), Some(t))
        }
        None => (iso.get(11..16).unwrap_or("--:--").to_string(), None),
    }
}

/// Bottom-right block: the next departures for a station, live from the DB
/// OpenBoards API (timetable and real-time forecast in a single request).
pub struct DeparturesModule {
    bounds: Rectangle,
    station_name: &'static str,
    cfg: DbConfig,
    schedule: Schedule,
    rows: Vec<Row>,
    /// The rows reflect a successful fetch on the current connection. When
    /// false (offline / API failing) the last rows are kept but flagged.
    live: bool,
    status: Option<String>,
}

impl DeparturesModule {
    pub fn new(bounds: Rectangle, station_name: &'static str, cfg: DbConfig) -> Self {
        Self {
            bounds,
            station_name,
            cfg,
            schedule: Schedule::default(),
            rows: Vec::new(),
            live: true,
            status: Some("Loading...".into()),
        }
    }

    /// POST a board request for the departures in the next `WINDOW_SECS`
    /// from `now` (Unix time) and reduce the reply to rows.
    fn fetch(&self, now: i64) -> anyhow::Result<Vec<Row>> {
        let body = serde_json::json!({
            "stopPlaceOrStation": { "evaNumbers": [self.cfg.eva] },
            "timeWindow": {
                "timeStart": util::rfc3339_utc(now),
                "timeEnd": util::rfc3339_utc(now + WINDOW_SECS),
            },
            "maxEntriesPerTransportType": FETCH_ROWS,
            "maxViaStops": 0,
            "languages": ["deu"],
        })
        .to_string();
        let headers = [
            ("DB-Client-Id", self.cfg.client_id.as_str()),
            ("DB-Api-Key", self.cfg.api_key.as_str()),
            ("Content-Type", "application/json"),
            ("Accept", "*/*"),
        ];
        let url = format!("{}/departures", self.cfg.base);
        let board: Board =
            util::fetch_json(Method::Post, &url, &headers, Some(body.as_bytes()))?;

        let mut rows: Vec<Row> = board
            .departures
            .iter()
            .map(|dep| Row::new(dep, &board.stop_places))
            .collect();
        rows.truncate(FETCH_ROWS);
        Ok(rows)
    }

    /// A compact fingerprint of the currently displayed board, used to decide
    /// whether the panel actually needs re-rendering.
    pub fn signature(&self) -> String {
        match &self.status {
            Some(s) => format!("!{s}"),
            None => self
                .rows
                .iter()
                .take(MAX_ROWS)
                .map(|r| {
                    format!(
                        "{}|{}|{}|{}|{}{};",
                        r.line, r.dest, r.planned, r.actual, r.delayed as u8, r.cancelled as u8
                    )
                })
                .chain([format!("live={}", self.live)])
                .collect(),
        }
    }
}

impl DisplayModule for DeparturesModule {
    fn bounds(&self) -> Rectangle {
        self.bounds
    }

    fn update(&mut self, ctx: &mut UpdateCtx) -> bool {
        // Live board: polled every tick; failures back off (see `Schedule`)
        // and keep the last rows on screen, flagged as not live. The time
        // window needs the real clock, so wait for SNTP.
        let before = self.signature();
        self.schedule.request();
        if ctx.online() && ctx.time_valid && self.schedule.due() {
            match self.fetch(util::unix_now()) {
                Ok(rows) => {
                    self.schedule.succeeded();
                    self.rows = rows;
                }
                Err(e) => {
                    error!("   Departures request failed: {e:#}");
                    self.schedule.failed();
                }
            }
        }
        self.live = ctx.online() && ctx.time_valid && !self.schedule.failing();

        // Live or stale, never show trains that have already left.
        if ctx.time_valid {
            let now = util::unix_now();
            self.rows.retain(|r| r.leaves.is_none_or(|t| t >= now));
        }

        self.status = match (self.rows.is_empty(), self.live) {
            (false, _) => None,
            (true, true) => Some("No departures".into()),
            (true, false) => Some("No live data".into()),
        };
        self.signature() != before
    }

    fn render(&self, display: &mut Display7in5) -> Result<(), Infallible> {
        let title = if self.live {
            format!("S-BAHN - {}", self.station_name)
        } else {
            format!("S-BAHN - {}  (not live)", self.station_name)
        };
        let inner = draw_panel(display, self.bounds, &title)?;
        let ix = inner.top_left.x;
        let iy = inner.top_left.y;
        let iw = inner.size.width as i32;

        let black = MonoTextStyleBuilder::new()
            .font(&FONT_9X15)
            .text_color(Color::Black)
            .build();
        let white_bold = MonoTextStyleBuilder::new()
            .font(&FONT_9X15_BOLD)
            .text_color(Color::White)
            .build();
        let top_left = TextStyleBuilder::new().baseline(Baseline::Top).build();
        let left_mid = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Left)
            .build();
        let center_mid = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        let fill = PrimitiveStyle::with_fill(Color::Black);
        let thin = PrimitiveStyle::with_stroke(Color::Black, 1);

        if let Some(status) = &self.status {
            Text::with_text_style(status, Point::new(ix, iy + 20), black, top_left)
                .draw(display)?;
            return Ok(());
        }

        const CW: i32 = 9; // FONT_9X15 advance width
        let rh = 29;
        let bh: u32 = 18;
        let time_w = 5 * CW; // width of "HH:MM"

        for (i, row) in self.rows.iter().take(MAX_ROWS).enumerate() {
            let ytop = iy + 2 + i as i32 * rh;
            let vmid = ytop + bh as i32 / 2;

            // --- Line badge: filled rounded box with inverted text (DB style) ---
            let n = row.line.chars().count().max(1) as i32;
            let bw = (n * CW + 12) as u32;
            rounded_box(display, Point::new(ix, ytop), Size::new(bw, bh), &fill)?;
            Text::with_text_style(
                &row.line,
                Point::new(ix + bw as i32 / 2, vmid),
                white_bold,
                center_mid,
            )
            .draw(display)?;

            // --- Right cluster: planned time, plus actual-time box when delayed ---
            let show_box = row.delayed || row.cancelled;
            let box_w = (time_w + 8) as u32;
            let cluster_w = if show_box {
                time_w + 8 + box_w as i32
            } else {
                time_w
            };
            let cluster_left = ix + iw - cluster_w;

            Text::with_text_style(
                &row.planned,
                Point::new(cluster_left, vmid),
                black,
                left_mid,
            )
            .draw(display)?;
            if row.cancelled {
                Line::new(
                    Point::new(cluster_left, vmid),
                    Point::new(cluster_left + time_w, vmid),
                )
                .draw_styled(&thin, display)?;
            }
            if show_box {
                let box_x = cluster_left + time_w + 8;
                rounded_box(
                    display,
                    Point::new(box_x, ytop),
                    Size::new(box_w, bh),
                    &fill,
                )?;
                let text = if row.cancelled {
                    "ausf."
                } else {
                    row.actual.as_str()
                };
                Text::with_text_style(
                    text,
                    Point::new(box_x + box_w as i32 / 2, vmid),
                    white_bold,
                    center_mid,
                )
                .draw(display)?;
            }

            // --- Destination between badge and time cluster ---
            let dest_x = ix + bw as i32 + 8;
            let dest_px = (cluster_left - 6 - dest_x).max(CW);
            let dest_chars = (dest_px / CW) as usize;
            Text::with_text_style(
                &truncate(&row.dest, dest_chars),
                Point::new(dest_x, vmid),
                black,
                left_mid,
            )
            .draw(display)?;
        }

        Ok(())
    }
}

/// Draw a filled rounded rectangle (used for the DB-style line and delay boxes).
fn rounded_box(
    display: &mut Display7in5,
    top_left: Point,
    size: Size,
    style: &PrimitiveStyle<Color>,
) -> Result<(), Infallible> {
    RoundedRectangle::with_equal_corners(Rectangle::new(top_left, size), Size::new(4, 4))
        .draw_styled(style, display)
}
