//! Shared plumbing for the data modules: HTTP fetch helpers and local-time
//! formatting. These used to live in `main`; modules now own their own network
//! logic and reach for these utilities instead.

use esp_idf_svc::http::{client::EspHttpConnection, Method};

/// Broken-down time type from the ESP-IDF C library.
pub type EspTm = esp_idf_svc::sys::tm;

/// Perform an HTTPS GET with JSON headers and return the body as a string.
pub fn http_get(client: &mut EspHttpConnection, url: &str) -> anyhow::Result<String> {
    let headers = [
        ("Accept", "application/json"),
        ("User-Agent", "smart-home-display-esp32"),
    ];
    client.initiate_request(Method::Get, url, &headers)?;
    client.initiate_response()?;
    read_body(client)
}

/// Drain the current HTTP response body into a (lossy) UTF-8 string.
pub fn read_body(client: &mut EspHttpConnection) -> anyhow::Result<String> {
    let mut buffer = [0u8; 1024];
    let mut body = Vec::new();
    while let Ok(read) = client.read(&mut buffer) {
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
        if body.len() > 262_144 {
            break; // safety cap
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Configure the C library timezone to Europe/Berlin (POSIX TZ with DST rules).
pub fn configure_timezone() {
    unsafe {
        esp_idf_svc::sys::setenv(
            c"TZ".as_ptr(),
            c"CET-1CEST,M3.5.0,M10.5.0/3".as_ptr(),
            1,
        );
        esp_idf_svc::sys::tzset();
    }
}

/// Local (date, time) strings, e.g. ("Wed, 20 Aug 2026", "14:35").
pub fn local_datetime() -> (String, String) {
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MO: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let tm = local_tm(0);
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

/// Broken-down local time, `offset_secs` from now (used for DB plan slots).
pub fn local_tm(offset_secs: i64) -> EspTm {
    use std::time::{SystemTime, UNIX_EPOCH};
    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let secs = (base + offset_secs) as esp_idf_svc::sys::time_t;
    let mut tm: EspTm = unsafe { core::mem::zeroed() };
    unsafe {
        esp_idf_svc::sys::localtime_r(&secs, &mut tm);
    }
    tm
}

/// `YYMMDD` for the DB `/plan/{eva}/{date}/{hour}` endpoint.
pub fn db_date(tm: &EspTm) -> String {
    format!(
        "{:02}{:02}{:02}",
        (tm.tm_year + 1900).rem_euclid(100),
        tm.tm_mon + 1,
        tm.tm_mday
    )
}

/// `HH` for the DB `/plan/{eva}/{date}/{hour}` endpoint.
pub fn db_hour(tm: &EspTm) -> String {
    format!("{:02}", tm.tm_hour)
}

/// `YYMMDDHHMM` timestamp used to filter and sort departures.
pub fn db_stamp(tm: &EspTm) -> String {
    format!("{}{}{:02}", db_date(tm), db_hour(tm), tm.tm_min)
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

/// Current UTC time as an RFC3339 timestamp (e.g. `2026-08-20T12:34:56Z`).
pub fn rfc3339_utc_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0) as esp_idf_svc::sys::time_t;
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
