use async_trait::async_trait;
use serde::Deserialize;

use crate::error::{DataError, NetworkError, WeatherError};
use crate::weather::provider::{WeatherProvider, WeatherProviderResponse};
use crate::weather::types::{CelestialEvents, WindSpeedUnit};
use crate::weather::units::normalize_temperature;
use crate::weather::WeatherLocation;
use crate::weather::WeatherUnits;

const BASE_URL: &str = "https://60s.viki.moe";

pub struct SixtyProvider {
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct SixtyResponse {
    code: i32,
    data: SixtyData,
}

#[derive(Deserialize)]
struct SixtyData {
    weather: SixtyWeather,
    sunrise: Option<SixtySunrise>,
}

#[derive(Deserialize)]
struct SixtyWeather {
    #[serde(rename = "condition_code")]
    condition_code: String,
    temperature: f64,
    #[serde(default)]
    precipitation: f64,
    wind_direction: String,
    wind_power: String,
}

#[derive(Deserialize)]
struct SixtySunrise {
    sunrise: Option<String>,
    sunset: Option<String>,
}

fn tencent_code_to_wmo(code: &str) -> i32 {
    match code {
        "00" => 0,
        "01" => 2,
        "02" => 3,
        "03" => 80,
        "04" => 95,
        "05" => 96,
        "06" => 66,
        "07" => 61,
        "08" => 63,
        "09" | "10" | "11" | "12" => 65,
        "13" => 85,
        "14" => 71,
        "15" => 73,
        "16" | "17" => 75,
        "18" => 45,
        "19" => 66,
        "20" | "29" | "30" | "31" | "32" => 48,
        "49" => 45,
        _ => 0,
    }
}

fn wind_dir_to_degrees(dir: &str) -> f64 {
    match dir {
        "北风" => 0.0,
        "东北风" => 45.0,
        "东风" => 90.0,
        "东南风" => 135.0,
        "南风" => 180.0,
        "西南风" => 225.0,
        "西风" => 270.0,
        "西北风" => 315.0,
        "北西北风" => 337.5,
        "北东北风" => 22.5,
        "东东北风" => 67.5,
        "东东南风" => 112.5,
        "南东南风" => 157.5,
        "南西南风" => 202.5,
        "西西南风" => 247.5,
        "西西北风" => 292.5,
        _ => 0.0,
    }
}

fn wind_power_to_kmh(power: &str) -> f64 {
    let parts: Vec<&str> = power.split('-').collect();
    let high = parts
        .last()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(3.0);
    high * 6.0 + 3.0
}

impl SixtyProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                eprintln!("警告: 创建 HTTP 客户端失败: {}", e);
                reqwest::Client::new()
            });
        Self { client }
    }

    fn build_url(&self, location: &WeatherLocation) -> String {
        format!(
            "{}?query={},{}",
            format!("{}/v2/weather", BASE_URL),
            location.latitude,
            location.longitude
        )
    }
}

impl Default for SixtyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WeatherProvider for SixtyProvider {
    fn get_attribution(&self) -> &'static str {
        "60s.胡.fun"
    }

    async fn get_current_weather(
        &self,
        location: &WeatherLocation,
        units: &WeatherUnits,
    ) -> Result<WeatherProviderResponse, WeatherError> {
        let url = self.build_url(location);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .and_then(|resp| resp.error_for_status())
            .map_err(|e| WeatherError::Network(NetworkError::from_reqwest(e, &url, 30)))?;

        let data: SixtyResponse = response
            .json()
            .await
            .map_err(|e| WeatherError::Network(NetworkError::from_reqwest(e, &url, 30)))?;

        if data.code != 200 {
            return Err(WeatherError::Data(DataError::BadData(format!(
                "API returned code {}",
                data.code
            ))));
        }

        let weather_code = tencent_code_to_wmo(&data.data.weather.condition_code);
        let wind_speed = wind_power_to_kmh(&data.data.weather.wind_power);

        let (sunrise_time, sunset_time) = match &data.data.sunrise {
            Some(s) => (s.sunrise.clone(), s.sunset.clone()),
            None => (None, None),
        };

        let is_day = match (&sunrise_time, &sunset_time) {
            (Some(r), Some(s)) => {
                use chrono::NaiveDateTime;
                let now = chrono::Local::now().naive_local();
                let rise = NaiveDateTime::parse_from_str(r, "%Y-%m-%d %H:%M:%S").ok();
                let set = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok();
                match (rise, set) {
                    (Some(rise), Some(set)) => now >= rise && now <= set,
                    _ => true,
                }
            }
            _ => true,
        };

        Ok(WeatherProviderResponse {
            weather_code,
            temperature: normalize_temperature(data.data.weather.temperature, units.temperature),
            precipitation: data.data.weather.precipitation,
            wind_speed: normalize_wind_speed_simple(wind_speed, units.wind_speed),
            wind_direction: wind_dir_to_degrees(&data.data.weather.wind_direction),
            sun: CelestialEvents::from_bool(is_day),
            moon_phase: Some(0.5),
            timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            attribution: self.get_attribution().to_string(),
            daily_high: None,
            daily_low: None,
            condition_duration_hours: None,
            next_condition: None,
            next_condition_start: None,
        })
    }
}

fn normalize_wind_speed_simple(speed: f64, unit: WindSpeedUnit) -> f64 {
    match unit {
        WindSpeedUnit::Kmh => speed,
        WindSpeedUnit::Ms => speed / 3.6,
        WindSpeedUnit::Mph => speed / 1.609,
        WindSpeedUnit::Kn => speed / 1.852,
    }
}
