use esp_idf_svc::http::client::EspHttpConnection;
use serde::Deserialize;

/// Instantaneous "current" block from Open-Meteo.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct Current {
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub temperature_2m: f32,
    #[serde(default)]
    pub relative_humidity_2m: Option<f32>,
    #[serde(default)]
    pub apparent_temperature: Option<f32>,
    #[serde(default)]
    pub weather_code: u16,
    #[serde(default)]
    pub wind_speed_10m: Option<f32>,
    #[serde(default)]
    pub wind_direction_10m: Option<f32>,
}

/// Hourly time-series (temperature, rain, probability, condition).
#[derive(Deserialize, Debug, Default, Clone)]
pub struct Hourly {
    #[serde(default)]
    pub time: Vec<String>,
    #[serde(default)]
    pub temperature_2m: Vec<f32>,
    #[serde(default)]
    pub rain: Vec<f32>,
    #[serde(default)]
    pub precipitation_probability: Vec<Option<u8>>,
    #[allow(dead_code)]
    #[serde(default)]
    pub weather_code: Vec<u16>,
}

/// Daily aggregates used for the 3-day forecast and sun times.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct Daily {
    #[serde(default)]
    pub time: Vec<String>,
    #[serde(default)]
    pub weather_code: Vec<u16>,
    #[serde(default)]
    pub temperature_2m_max: Vec<f32>,
    #[serde(default)]
    pub temperature_2m_min: Vec<f32>,
    #[serde(default)]
    pub sunrise: Vec<String>,
    #[serde(default)]
    pub sunset: Vec<String>,
    #[serde(default)]
    pub uv_index_max: Vec<Option<f32>>,
    #[serde(default)]
    pub precipitation_probability_max: Vec<Option<u8>>,
    /// Daily sunshine duration in seconds.
    #[serde(default)]
    pub sunshine_duration: Vec<f32>,
    /// Total daily precipitation in mm.
    #[serde(default)]
    pub precipitation_sum: Vec<f32>,
}

/// The full Open-Meteo response, shared (via `Rc`) between the weather modules.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct WeatherData {
    #[serde(default)]
    pub current: Option<Current>,
    #[serde(default)]
    pub hourly: Hourly,
    #[serde(default)]
    pub daily: Option<Daily>,
}

impl WeatherData {
    /// Fetch the Open-Meteo forecast and parse it in one shot. One request
    /// feeds the current-conditions, 24h-graph and 3-day modules.
    pub fn fetch(client: &mut EspHttpConnection, url: &str) -> anyhow::Result<Self> {
        let body = super::util::http_get(client, url)?;
        Self::parse(&body).map_err(|_| anyhow::anyhow!("failed to parse weather JSON"))
    }

    pub fn parse(json: &str) -> Result<Self, ()> {
        serde_json::from_str::<WeatherData>(json).map_err(|_| ())
    }

    /// Index into the hourly arrays that corresponds to the current hour, found
    /// by matching the `current.time` prefix (`YYYY-MM-DDTHH`) against
    /// `hourly.time`. Falls back to 0 when it cannot be determined.
    pub fn current_hour_index(&self) -> usize {
        if let Some(cur) = &self.current {
            if let Some(key) = cur.time.get(..13) {
                if let Some(i) = self
                    .hourly
                    .time
                    .iter()
                    .position(|t| t.get(..13) == Some(key))
                {
                    return i;
                }
            }
        }
        0
    }
}

/// Short human-readable label for a WMO weather code.
pub fn wmo_text(code: u16) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snowfall",
        77 => "Snow grains",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, hail",
        _ => "Unknown",
    }
}
