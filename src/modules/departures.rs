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
use esp_idf_svc::http::{client::EspHttpConnection, Method};
use log::error;

use std::collections::{HashMap, HashSet};

use super::{DisplayModule, UpdateCtx};
use crate::helpers::{draw_panel, truncate, util};

const MAX_ROWS: usize = 6;

/// DB Timetables API access, injected at construction from `.secret`.
pub struct DbConfig {
    pub base: String,
    pub eva: String,
    pub client_id: String,
    pub api_key: String,
}

// ---- DB Timetables API (XML) ----

/// A flattened departure ready for rendering, DB-board style.
struct Row {
    line: String,
    dest: String,
    planned: String,
    actual: String,
    delayed: bool,
    cancelled: bool,
    /// Effective departure timestamp (`YYMMDDHHMM`), used to sort and filter.
    sort_key: String,
}

/// Bottom-right block: the next departures for a station, from the DB
/// Timetables API (planned timetable merged with the changes feed).
pub struct DeparturesModule {
    bounds: Rectangle,
    station_name: &'static str,
    cfg: DbConfig,
    rows: Vec<Row>,
    status: Option<String>,
}

impl DeparturesModule {
    pub fn new(bounds: Rectangle, station_name: &'static str, cfg: DbConfig) -> Self {
        Self {
            bounds,
            station_name,
            cfg,
            rows: Vec::new(),
            status: Some("Loading...".into()),
        }
    }

    /// GET against the DB Timetables API (XML body, credentialed headers).
    fn db_get(&self, client: &mut EspHttpConnection, url: &str) -> anyhow::Result<String> {
        let headers = [
            ("DB-Client-Id", self.cfg.client_id.as_str()),
            ("DB-Api-Key", self.cfg.api_key.as_str()),
            ("Accept", "application/xml"),
        ];
        client.initiate_request(Method::Get, url, &headers)?;
        client.initiate_response()?;
        util::read_body(client)
    }

    /// Build departure rows from one or more planned-timetable documents
    /// (`/plan/...`) merged with the changes feed (`/fchg/...`), keeping only
    /// departures at or after `now` (a `YYMMDDHHMM` timestamp).
    pub fn load(&mut self, plans: &[&str], changes: &str, now: &str) -> Result<(), ()> {
        // 1) Delays / cancellations keyed by trip id.
        let mut change_map: HashMap<String, (Option<String>, bool)> = HashMap::new();
        for_each_s(changes, |block| {
            let id = match open_tag(block, "<s").and_then(|t| attr(t, "id")) {
                Some(v) => v.to_string(),
                None => return,
            };
            if let Some(dp) = open_tag(block, "<dp") {
                let ct = attr(dp, "ct").map(str::to_string);
                let cancelled = attr(dp, "cs") == Some("c");
                change_map.insert(id, (ct, cancelled));
            }
        });

        // 2) Planned departures, with changes applied.
        let mut rows: Vec<Row> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for plan in plans {
            for_each_s(plan, |block| {
                let id = match open_tag(block, "<s").and_then(|t| attr(t, "id")) {
                    Some(v) => v.to_string(),
                    None => return,
                };
                let dp = match open_tag(block, "<dp") {
                    Some(v) => v,
                    None => return, // arrival-only stop, no departure here
                };
                let pt = match attr(dp, "pt") {
                    Some(v) => v,
                    None => return,
                };

                let (ct, cancelled) = match change_map.get(&id) {
                    Some((c, x)) => (c.clone(), *x),
                    None => (None, false),
                };
                let effective = ct.clone().unwrap_or_else(|| pt.to_string());
                if effective.as_str() < now || !seen.insert(id) {
                    return; // already departed, or a duplicate across hours
                }

                let line: String = attr(dp, "l").unwrap_or("").split_whitespace().collect();
                let dest = last_stop(attr(dp, "ppth").unwrap_or(""));
                let delayed = !cancelled && ct.as_deref().map(|c| c != pt).unwrap_or(false);

                rows.push(Row {
                    line: if line.is_empty() { "?".into() } else { line },
                    dest: if dest.is_empty() { "-".into() } else { dest },
                    planned: hhmm_db(pt),
                    actual: hhmm_db(&effective),
                    delayed,
                    cancelled,
                    sort_key: effective,
                });
            });
        }

        rows.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
        rows.truncate(MAX_ROWS);

        self.rows = rows;
        self.status = if self.rows.is_empty() {
            Some("No departures".into())
        } else {
            None
        };
        Ok(())
    }

    /// A compact fingerprint of the currently displayed board, used to decide
    /// whether the panel actually needs re-rendering.
    pub fn signature(&self) -> String {
        match &self.status {
            Some(s) => format!("!{s}"),
            None => self
                .rows
                .iter()
                .map(|r| {
                    format!(
                        "{}|{}|{}|{}|{}{};",
                        r.line, r.dest, r.planned, r.actual, r.delayed as u8, r.cancelled as u8
                    )
                })
                .collect(),
        }
    }
}

// ---- minimal XML attribute scanning (no external XML dependency) ----

/// Invoke `f` for every `<s>...</s>` trip element in the document.
fn for_each_s<F: FnMut(&str)>(xml: &str, mut f: F) {
    let mut rest = xml;
    while let Some(start) = rest.find("<s ") {
        let after = &rest[start..];
        let end = after.find("</s>").map(|e| e + 4).unwrap_or(after.len());
        f(&after[..end]);
        rest = &after[end..];
    }
}

/// The opening-tag substring (up to but excluding `>`) that starts with
/// `prefix` (e.g. `"<dp"`), or `None` if absent.
fn open_tag<'a>(block: &'a str, prefix: &str) -> Option<&'a str> {
    let start = block.find(prefix)?;
    let rest = &block[start..];
    let end = rest.find('>')?;
    Some(&rest[..end])
}

/// Read a double-quoted attribute value from a single opening tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Final destination = last entry of a `|`-separated path list (`ppth`).
fn last_stop(ppth: &str) -> String {
    ppth.rsplit('|').next().unwrap_or("").trim().to_string()
}

/// `HH:MM` from a DB timestamp `YYMMDDHHMM` (e.g. `2608201632` -> `16:32`).
fn hhmm_db(stamp: &str) -> String {
    if stamp.len() >= 10 {
        format!("{}:{}", &stamp[6..8], &stamp[8..10])
    } else {
        "--:--".to_string()
    }
}

impl DisplayModule for DeparturesModule {
    fn update(&mut self, ctx: &mut UpdateCtx) -> bool {
        // Planned timetable for the current and next hour, merged with the live
        // changes feed. Polled every tick; reports a change only when the board
        // content actually differs.
        let now_stamp = util::db_stamp(&ctx.now);
        let mut plans: Vec<String> = Vec::new();
        for tm in [&ctx.now, &ctx.next_hour] {
            let url = format!(
                "{}/plan/{}/{}/{}",
                self.cfg.base,
                self.cfg.eva,
                util::db_date(tm),
                util::db_hour(tm)
            );
            match self.db_get(ctx.client, &url) {
                Ok(body) => plans.push(body),
                Err(e) => error!("   Plan request failed: {e:?}"),
            }
        }
        let changes = match self.db_get(
            ctx.client,
            &format!("{}/fchg/{}", self.cfg.base, self.cfg.eva),
        ) {
            Ok(body) => body,
            Err(e) => {
                error!("   Changes request failed: {e:?}");
                String::new()
            }
        };
        let plan_refs: Vec<&str> = plans.iter().map(String::as_str).collect();
        let prev = self.signature();
        if self.load(&plan_refs, &changes, &now_stamp).is_err() {
            error!("   Failed to parse departures.");
            return false;
        }
        self.signature() != prev
    }

    fn render(&self, display: &mut Display7in5) -> Result<(), Infallible> {
        let title = format!("S-BAHN - {}", self.station_name);
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

        for (i, row) in self.rows.iter().enumerate() {
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
