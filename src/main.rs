mod controller;
mod display;
mod modules;

use crate::controller::DisplayController;
use crate::modules::weather::WeatherModule;

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::peripherals::Peripherals,
    http::{
        client::{Configuration as HttpConfig, EspHttpConnection},
        Method,
    },
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use log::{error, info};
use std::time::Duration;

const SSID: &str = "";
const PASSWORD: &str = "";
const WEATHER_URL: &str = "https://api.open-meteo.com/v1/forecast?latitude=49.591&longitude=11.0078&hourly=temperature_2m,rain,precipitation_probability,weather_code&timezone=Europe%2FBerlin";

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
    info!("2. WI-FI CONNECTED SUCCESSFULLY!");

    info!("3. Initializing E-Paper display...");
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

    info!("4. Dropping into Main Weather Loop...");

    // Main UI Loop (Refreshes every 30 minutes)
    loop {
        info!("Fetching latest weather data...");

        client.initiate_request(Method::Get, WEATHER_URL, &[])?;
        client.initiate_response()?;

        let mut buffer = [0u8; 2048];
        let mut response_body = Vec::new();
        while let Ok(read) = client.read(&mut buffer) {
            if read == 0 {
                break;
            }
            response_body.extend_from_slice(&buffer[..read]);
        }

        if let Ok(json_str) = std::str::from_utf8(&response_body) {
            // Create a fresh weather module and parse the new JSON payload
            let mut weather_mod = WeatherModule::new(Duration::from_secs(1800));

            if weather_mod.load_json(json_str).is_ok() {
                info!("Data successfully parsed! Rendering to screen...");
                controller.clear_modules();
                controller.register(Box::new(weather_mod));
                controller.force_render();
            } else {
                error!("Failed to parse JSON payload!");
            }
        }

        // Sleep for 30 minutes before fetching again
        std::thread::sleep(Duration::from_secs(1800));
    }
}
