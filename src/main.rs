mod controller;
mod display;
mod helpers;
mod modules;

use core::time::Duration;

use crate::controller::DisplayController;
use crate::helpers::util::configure_timezone;
use crate::modules::{
    calendar::{CalendarModule, GcalConfig},
    current_weather::CurrentWeatherModule,
    daily_forecast::DailyForecastModule,
    departures::{DbConfig, DeparturesModule},
    forecast_graph::ForecastGraphModule,
    header::HeaderModule,
    DisplayModule, UpdateCtx,
};

use anyhow::anyhow;
use embedded_graphics::{prelude::*, primitives::Rectangle};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        peripherals::Peripherals,
        reset,
        task::watchdog::{TWDTConfig, TWDTDriver},
    },
    nvs::EspDefaultNvsPartition,
    sntp::{EspSntp, SyncStatus},
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use log::{error, info, warn};

// Credentials come from the gitignored `.secret` file, injected at build time
// (see build.rs) as compile-time environment variables.
const SSID: &str = env!("SECRET_WIFI_SSID");
const PASSWORD: &str = env!("SECRET_WIFI_PASSWORD");

// One Open-Meteo request feeds the current-conditions, 24h-graph and 3-day
// blocks. Coordinates come from the gitignored `.secret` file (see build.rs);
const WEATHER_URL: &str = concat!(
    "https://api.open-meteo.com/v1/forecast?latitude=",
    env!("SECRET_WEATHER_LAT"),
    "&longitude=",
    env!("SECRET_WEATHER_LON"),
    "&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m,wind_direction_10m",
    "&hourly=temperature_2m,rain,precipitation_probability,weather_code",
    "&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,uv_index_max,precipitation_probability_max,sunshine_duration,precipitation_sum",
    "&timezone=Europe%2FBerlin&forecast_days=4&wind_speed_unit=kmh"
);

// DB OpenBoards API (DB API Marketplace). Credentials + station EVA from `.secret`.
const SBAHN_STATION: &str = "Erlangen-Bruck";
const DB_BASE: &str = "https://apis.deutschebahn.com/db-api-marketplace/apis/open-boards/v1";
const DB_CLIENT_ID: &str = env!("SECRET_DB_CLIENT_ID");
const DB_API_KEY: &str = env!("SECRET_DB_API_KEY");
const DB_EVA_NO: &str = env!("SECRET_DB_EVA_NO");

// Google Calendar (OAuth installed-app flow). All from `.secret`; leave the
// refresh token empty to disable the calendar block. Calendar id defaults to
// "primary" when unset.
const GCAL_CLIENT_ID: &str = env!("SECRET_GCAL_CLIENT_ID");
const GCAL_CLIENT_SECRET: &str = env!("SECRET_GCAL_CLIENT_SECRET");
const GCAL_REFRESH_TOKEN: &str = env!("SECRET_GCAL_REFRESH_TOKEN");
const GCAL_CALENDAR_ID: &str = env!("SECRET_GCAL_CALENDAR_ID");

// The panel is only refreshed on real changes: weather (and therefore the
// header clock) on a slow 30-minute cadence, while S-Bahn + calendar are polled
// every minute and trigger a redraw only when their content actually changes.
const POLL_SECS: u32 = 60;
const DISPLAY_REFRESH_SECS: u32 = 1800; // full weather/clock refresh: 30 minutes
const WEATHER_EVERY: u32 = DISPLAY_REFRESH_SECS / POLL_SECS;

/// The task watchdog reboots the board when the main loop stops feeding it for
/// this long: a panel whose BUSY line never clears, a wedged network call, ...
/// Well above the slowest legitimate step between feeds (one module update
/// with two HTTP requests on a slow network).
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(180);

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // `run` only returns on a setup failure. Returning from main would leave
    // the board idle with a frozen panel, so start over instead.
    if let Err(e) = run() {
        error!("Fatal: {e:#}");
    }
    error!("Restarting in 10 s...");
    FreeRtos::delay_ms(10_000);
    reset::restart();
}

fn run() -> anyhow::Result<()> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut twdt = TWDTDriver::new(
        peripherals.twdt,
        &TWDTConfig {
            duration: WATCHDOG_TIMEOUT,
            panic_on_trigger: true,
            ..Default::default()
        },
    )?;
    let mut watchdog = twdt.watch_current_task()?;

    // The display goes first so its 48 KB framebuffer is allocated in one
    // piece before Wi-Fi and TLS fragment the heap.
    info!("1. Initializing E-Paper display...");
    let hw = display::setup_hardware(
        peripherals.spi2,
        peripherals.pins.gpio13.into(), // SCLK
        peripherals.pins.gpio14.into(), // MOSI
        peripherals.pins.gpio15.into(), // CS
        peripherals.pins.gpio25.into(), // BUSY
        peripherals.pins.gpio26.into(), // RST
        peripherals.pins.gpio27.into(), // DC
    )?;
    let mut controller = DisplayController::new(hw);

    info!("2. Starting Wi-Fi...");
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID
            .try_into()
            .map_err(|_| anyhow!("WIFI_SSID is longer than 32 bytes"))?,
        password: PASSWORD
            .try_into()
            .map_err(|_| anyhow!("WIFI_PASSWORD is longer than 64 bytes"))?,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    }))?;
    wifi.start()?;
    // Not fatal: without a network the dashboard starts offline and retries.
    let online = ensure_wifi(&mut wifi).is_some();

    // Real, local time: set the Europe/Berlin timezone and sync via SNTP. SNTP
    // keeps retrying in the background, so an offline boot syncs later.
    configure_timezone();
    let sntp = EspSntp::new_default()?;
    if online {
        info!("3. Waiting for SNTP time sync...");
        let mut tries = 0;
        while sntp.get_sync_status() != SyncStatus::Completed && tries < 200 {
            FreeRtos::delay_ms(100);
            tries += 1;
        }
        info!("   SNTP status: {:?}", sntp.get_sync_status());
    }

    // ---- Set up all dashboard modules for the 800x480 panel ----
    // Header on top; left column = current weather, 3-day forecast, and a
    // half-width Google Calendar at the bottom; right column = 24h graph over
    // S-Bahn departures, split 50/50.
    let mut header = HeaderModule::new(Rectangle::new(Point::new(0, 0), Size::new(800, 52)));
    let mut current = CurrentWeatherModule::new(
        Rectangle::new(Point::new(2, 56), Size::new(396, 152)),
        WEATHER_URL.to_string(),
    );
    let mut daily = DailyForecastModule::new(
        Rectangle::new(Point::new(2, 212), Size::new(396, 132)),
        WEATHER_URL.to_string(),
    );
    let mut calendar = CalendarModule::new(
        Rectangle::new(Point::new(2, 348), Size::new(396, 130)),
        GcalConfig {
            client_id: GCAL_CLIENT_ID.to_string(),
            client_secret: GCAL_CLIENT_SECRET.to_string(),
            refresh_token: GCAL_REFRESH_TOKEN.to_string(),
            calendar_id: GCAL_CALENDAR_ID.to_string(),
        },
    );
    let mut graph = ForecastGraphModule::new(
        Rectangle::new(Point::new(402, 56), Size::new(396, 209)),
        WEATHER_URL.to_string(),
    );
    let mut sbahn = DeparturesModule::new(
        Rectangle::new(Point::new(402, 269), Size::new(396, 209)),
        SBAHN_STATION,
        DbConfig {
            base: DB_BASE.to_string(),
            eva: DB_EVA_NO.to_string(),
            client_id: DB_CLIENT_ID.to_string(),
            api_key: DB_API_KEY.to_string(),
        },
    );

    info!("4. Entering main dashboard loop...");
    let mut tick: u32 = 0;
    loop {
        let _ = watchdog.feed();
        // Reconnect if the link dropped; while offline the modules keep (and
        // flag) their last data and retry once the network is back.
        let ip = ensure_wifi(&mut wifi);

        let slow = tick.is_multiple_of(WEATHER_EVERY);
        // Full flash at boot, every 30 min (clears partial-refresh ghosting)
        // and to recover from a failed refresh.
        let full_refresh = tick == 0 || slow || controller.needs_full_refresh();
        let mut changed = [false; 6];
        {
            let mut ctx = UpdateCtx::new(ip, slow);
            let mut updatables: [&mut dyn DisplayModule; 6] = [
                &mut header,
                &mut current,
                &mut graph,
                &mut daily,
                &mut sbahn,
                &mut calendar,
            ];
            for (i, module) in updatables.iter_mut().enumerate() {
                let _ = watchdog.feed();
                changed[i] = module.update(&mut ctx);
            }
        }
        let _ = watchdog.feed();

        // Refresh the panel only when something actually changed. The module
        // array must stay in the same order as `updatables` above so `changed`
        // lines up index-for-index.
        let modules: [&dyn DisplayModule; 6] =
            [&header, &current, &graph, &daily, &sbahn, &calendar];
        let refresh = if full_refresh {
            Some(("Full", controller.render(&modules)))
        } else if changed.iter().any(|&c| c) {
            Some(("Partial", controller.render_partial(&modules, &changed)))
        } else {
            None
        };
        match refresh {
            Some((kind, Ok(()))) => info!("   {kind} frame rendered."),
            // Not fatal: the next tick resets the panel and redraws it fully.
            Some((_, Err(e))) => error!("   Display refresh failed: {e:#}"),
            None => {}
        }

        tick = tick.wrapping_add(1);
        let _ = watchdog.feed();
        FreeRtos::delay_ms(POLL_SECS * 1000);
    }
}

/// Make sure the station is associated and has an IP, reconnecting when the
/// link dropped (ESP-IDF does not reconnect on its own). Never fails: errors
/// are logged and the dashboard carries on offline, retrying next tick.
/// Returns the device IP while online.
fn ensure_wifi(wifi: &mut BlockingWifi<EspWifi<'_>>) -> Option<String> {
    if !wifi.is_up().unwrap_or(false) {
        info!("   Wi-Fi down, connecting...");
        // Drop any half-open association first (errors just mean there was none).
        let _ = wifi.wifi_mut().disconnect();
        if let Err(e) = wifi.connect().and_then(|()| wifi.wait_netif_up()) {
            warn!("   Wi-Fi connect failed: {e}");
            return None;
        }
        info!("   Wi-Fi connected.");
    }
    wifi.wifi()
        .sta_netif()
        .get_ip_info()
        .ok()
        .map(|info| info.ip.to_string())
}
