mod controller;
mod display;
mod helpers;
mod modules;

use crate::controller::DisplayController;
use crate::helpers::util::{configure_timezone, local_tm};
use crate::modules::{
    calendar::{CalendarModule, GcalConfig},
    current_weather::CurrentWeatherModule,
    daily_forecast::DailyForecastModule,
    departures::{DbConfig, DeparturesModule},
    forecast_graph::ForecastGraphModule,
    header::HeaderModule,
    DisplayModule, UpdateCtx,
};

use embedded_graphics::{prelude::*, primitives::Rectangle};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{delay::FreeRtos, peripherals::Peripherals},
    http::client::{Configuration as HttpConfig, EspHttpConnection},
    nvs::EspDefaultNvsPartition,
    sntp::{EspSntp, SyncStatus},
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use log::info;

// Credentials come from the gitignored `.secret` file, injected at build time
// (see build.rs) as compile-time environment variables.
const SSID: &str = env!("SECRET_WIFI_SSID");
const PASSWORD: &str = env!("SECRET_WIFI_PASSWORD");

// One Open-Meteo request feeds the current-conditions, 24h-graph and 3-day blocks.
const WEATHER_URL: &str = "https://api.open-meteo.com/v1/forecast?latitude=49.591&longitude=11.0078&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m,wind_direction_10m&hourly=temperature_2m,rain,precipitation_probability,weather_code&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,uv_index_max,precipitation_probability_max,sunshine_duration,precipitation_sum&timezone=Europe%2FBerlin&forecast_days=4&wind_speed_unit=kmh";

// DB Timetables API (DB API Marketplace). Credentials + station EVA from `.secret`.
const SBAHN_STATION: &str = "Erlangen-Bruck";
const DB_BASE: &str = "https://apis.deutschebahn.com/db-api-marketplace/apis/timetables/v1";
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

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    info!("1. Hardware initialized. Starting Wi-Fi...");
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: PASSWORD.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    info!("2. Wi-Fi connected.");

    // Real, local time: set the Europe/Berlin timezone and sync via SNTP.
    configure_timezone();
    let sntp = EspSntp::new_default()?;
    info!("3. Waiting for SNTP time sync...");
    let mut tries = 0;
    while sntp.get_sync_status() != SyncStatus::Completed && tries < 200 {
        FreeRtos::delay_ms(100);
        tries += 1;
    }
    info!("   SNTP status: {:?}", sntp.get_sync_status());

    info!("4. Initializing E-Paper display...");
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

    let http_config = HttpConfig {
        use_global_ca_store: true,
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let mut client = EspHttpConnection::new(&http_config)?;

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

    // The device IP is stable once Wi-Fi is up; set it once for the header.
    if let Ok(ip) = wifi.wifi().sta_netif().get_ip_info() {
        header.set_ip(ip.ip.to_string());
    }

    info!("5. Entering main dashboard loop...");
    let mut tick: u32 = 0;
    loop {
        // Each module fetches its own data and reports whether its content
        // changed. Weather (and the header clock) refresh on the slow 30-minute
        // cadence (`slow`); S-Bahn + calendar poll every minute.
        let slow = tick % WEATHER_EVERY == 0;
        let mut need_render = tick == 0;
        {
            let mut ctx = UpdateCtx {
                client: &mut client,
                slow,
                now: local_tm(0),
                next_hour: local_tm(3600),
                weather: None,
                weather_tried: false,
            };
            let mut updatables: [&mut dyn DisplayModule; 6] = [
                &mut header,
                &mut current,
                &mut graph,
                &mut daily,
                &mut sbahn,
                &mut calendar,
            ];
            for module in updatables.iter_mut() {
                if module.update(&mut ctx) {
                    need_render = true;
                }
            }
        }

        // Refresh the panel only when something actually changed.
        if need_render {
            let modules: [&dyn DisplayModule; 6] =
                [&header, &current, &graph, &daily, &sbahn, &calendar];
            controller.render(&modules);
            info!("   Frame rendered.");
        }

        tick = tick.wrapping_add(1);
        FreeRtos::delay_ms(POLL_SECS * 1000);
    }
}
