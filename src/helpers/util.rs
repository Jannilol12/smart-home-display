//! Shared plumbing for the data modules: HTTP fetch helpers, retry pacing and
//! local-time formatting. Modules own their own network logic and reach for
//! these utilities instead.

use std::io::{BufReader, Read};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use esp_idf_svc::http::{
    client::{Configuration as HttpConfig, EspHttpConnection},
    Method,
};
use serde::de::DeserializeOwned;

/// Broken-down time type from the ESP-IDF C library.
pub type EspTm = esp_idf_svc::sys::tm;

/// Network timeout per operation (connect, TLS handshake, each read/write).
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Perform one HTTPS request and deserialize its JSON response.
///
/// Every request runs on a fresh connection that is torn down afterwards.
/// `EspHttpConnection` must not be reused after a failure: a request that dies
/// between `initiate_request` and `initiate_response` (Wi-Fi drop, timeout)
/// leaves it stuck in the request phase and the next `initiate_request`
/// panics, while a half-read response makes the next request try to flush the
/// dead socket forever. Dropping the connection always cleans up, and also
/// frees the TLS session buffers between polls.
///
/// The body is parsed straight off the socket rather than collected into a
/// response-sized buffer first, so a large reply can't exhaust the fragmented
/// heap. Non-2xx statuses are reported as errors.
pub fn fetch_json<T: DeserializeOwned>(
    method: Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> anyhow::Result<T> {
    let mut conn = EspHttpConnection::new(&HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        // The ESP-IDF default (512 B) is too small for Google's response
        // headers and the long outgoing `Authorization: Bearer` header.
        buffer_size: Some(4096),
        buffer_size_tx: Some(4096),
        timeout: Some(HTTP_TIMEOUT),
        ..Default::default()
    })?;

    let content_len = body.map(|b| b.len().to_string());
    let mut all_headers = vec![("User-Agent", "smart-home-display-esp32")];
    all_headers.extend_from_slice(headers);
    if let Some(len) = &content_len {
        all_headers.push(("Content-Length", len.as_str()));
    }

    conn.initiate_request(method, url, &all_headers)?;
    if let Some(body) = body {
        conn.write_all(body)?;
    }
    conn.initiate_response()?;

    let status = conn.status();
    if !(200..300).contains(&status) {
        anyhow::bail!("HTTP {status}: {}", error_excerpt(&mut conn));
    }
    serde_json::from_reader(BufReader::with_capacity(1024, ResponseBody(&mut conn)))
        .map_err(|e| anyhow::anyhow!("bad JSON response: {e}"))
}

/// The start of an error response body, for the log (e.g. the API's reason).
fn error_excerpt(conn: &mut EspHttpConnection) -> String {
    let mut buf = [0u8; 300];
    let mut n = 0;
    while n < buf.len() {
        match conn.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(read) => n += read,
        }
    }
    String::from_utf8_lossy(&buf[..n]).trim().to_string()
}

/// `std::io::Read` over a response body, so serde_json can stream it.
struct ResponseBody<'a>(&'a mut EspHttpConnection);

impl Read for ResponseBody<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf).map_err(std::io::Error::other)
    }
}

/// First retry lands on the very next poll (just under one 60 s tick), then
/// the wait doubles up to `RETRY_MAX`.
const RETRY_BASE: Duration = Duration::from_secs(55);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);

/// Fetch pacing for one data source. A refresh is [`request`](Self::request)ed
/// on the source's regular cadence and stays pending until a fetch succeeds.
/// Failed attempts are retried with exponential back-off (next poll, then
/// after ~2, ~4 and at most every 5 minutes), so an outage is picked up again
/// quickly without stalling every poll on network timeouts. Polls skipped while
/// offline simply leave the request pending.
pub struct Schedule {
    pending: bool,
    failures: u32,
    retry_at: Option<Instant>,
}

impl Default for Schedule {
    /// Starts out pending, so the first poll fetches straight away.
    fn default() -> Self {
        Self {
            pending: true,
            failures: 0,
            retry_at: None,
        }
    }
}

impl Schedule {
    /// Ask for a refresh (idempotent).
    pub fn request(&mut self) {
        self.pending = true;
    }

    /// A refresh is pending and any back-off has elapsed.
    pub fn due(&self) -> bool {
        self.pending && self.retry_at.is_none_or(|t| Instant::now() >= t)
    }

    /// The last attempt failed, i.e. the data on screen is stale.
    pub fn failing(&self) -> bool {
        self.failures > 0
    }

    pub fn succeeded(&mut self) {
        self.pending = false;
        self.failures = 0;
        self.retry_at = None;
    }

    pub fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
        let factor = 1u32 << (self.failures - 1).min(4);
        self.retry_at = Some(Instant::now() + (RETRY_BASE * factor).min(RETRY_MAX));
    }
}

/// Configure the C library timezone to Europe/Berlin (POSIX TZ with DST rules).
pub fn configure_timezone() {
    unsafe {
        esp_idf_svc::sys::setenv(c"TZ".as_ptr(), c"CET-1CEST,M3.5.0,M10.5.0/3".as_ptr(), 1);
        esp_idf_svc::sys::tzset();
    }
}

/// Local (date, time) strings, e.g. ("Wed, 20 Aug 2026", "14:35").
pub fn local_datetime() -> (String, String) {
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MO: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let tm = local_tm(unix_now());
    let wd = WD
        .get(tm.tm_wday.rem_euclid(7) as usize)
        .copied()
        .unwrap_or("");
    let mo = MO
        .get(tm.tm_mon.rem_euclid(12) as usize)
        .copied()
        .unwrap_or("");
    let date = format!("{}, {:02} {} {}", wd, tm.tm_mday, mo, tm.tm_year + 1900);
    let time = format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
    (date, time)
}

/// Current Unix time in seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether SNTP has set the clock yet. Until then it counts up from 1970,
/// which must not end up on the panel or in API requests.
pub fn clock_is_set() -> bool {
    unix_now() >= 1_704_067_200 // 2024-01-01T00:00:00Z
}

/// Broken-down local time for a Unix timestamp.
pub fn local_tm(secs: i64) -> EspTm {
    let secs = secs as esp_idf_svc::sys::time_t;
    let mut tm: EspTm = unsafe { core::mem::zeroed() };
    unsafe {
        esp_idf_svc::sys::localtime_r(&secs, &mut tm);
    }
    tm
}

/// Unix time of an ISO-8601 timestamp with an explicit offset, e.g.
/// `2026-11-19T16:32:00+01:00` or `2026-11-19T15:32:00.000Z`.
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let num = |r: core::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi) = (num(11..13)?, num(14..16)?);
    let sec = if s.as_bytes().get(16) == Some(&b':') {
        num(17..19)?
    } else {
        0
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }

    // Offset after the time-of-day (and any fractional seconds).
    let tail = s.get(16..)?;
    let offset = if tail.ends_with('Z') {
        0
    } else {
        let pos = tail.rfind(['+', '-'])?;
        let off = &tail[pos + 1..]; // "HH:MM" or "HHMM"
        let oh: i64 = off.get(0..2)?.parse().ok()?;
        let om: i64 = off.get(off.len().checked_sub(2)?..)?.parse().ok()?;
        let secs = oh * 3600 + om * 60;
        if tail[pos..].starts_with('-') {
            -secs
        } else {
            secs
        }
    };

    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some(days * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

/// Local date as `YYYY-MM-DD` (used to decide the calendar's month display).
pub fn ymd(tm: &EspTm) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    )
}

/// A Unix time as an RFC3339 UTC timestamp (e.g. `2026-08-20T12:34:56Z`).
pub fn rfc3339_utc(secs: i64) -> String {
    let secs = secs as esp_idf_svc::sys::time_t;
    let mut tm: EspTm = unsafe { core::mem::zeroed() };
    unsafe {
        esp_idf_svc::sys::gmtime_r(&secs, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}
