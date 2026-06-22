use crate::error::{NetworkError, WeatherError};
use crate::weather::provider::{WeatherProvider, WeatherProviderResponse};
use crate::weather::types::{
    CelestialEvents, PrecipitationUnit, TemperatureUnit, WeatherLocation, WeatherUnits,
    WindSpeedUnit,
};
use crate::weather::units::{normalize_precipitation, normalize_temperature, normalize_wind_speed};
use async_trait::async_trait;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use std::time::Duration;

const OPEN_METEO_BASE_URL: &str = "https://api.open-meteo.com/v1/forecast";

pub struct OpenMeteoProvider {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    current: CurrentWeather,
    #[serde(default)]
    daily: Option<DailyData>,
    #[serde(default)]
    hourly: Option<HourlyData>,
}

#[derive(Debug, Deserialize)]
struct CurrentWeather {
    time: String,
    temperature_2m: f64,
    #[serde(deserialize_with = "deserialize_i32_from_number")]
    is_day: i32,
    precipitation: f64,
    #[serde(deserialize_with = "deserialize_i32_from_number")]
    weather_code: i32,
    wind_speed_10m: f64,
    wind_direction_10m: f64,
}

#[derive(Debug, Deserialize)]
struct DailyData {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct HourlyData {
    time: Vec<String>,
    weather_code: Vec<Option<f64>>,
}

fn deserialize_i32_from_number<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Integer(i32),
        Float(f64),
    }

    match Number::deserialize(deserializer)? {
        Number::Integer(value) => Ok(value),
        Number::Float(value) => {
            if !value.is_finite() {
                return Err(de::Error::custom("expected a finite numeric value"));
            }
            Ok(value.round() as i32)
        }
    }
}

fn hourly_codes_to_i32(codes: &[Option<f64>]) -> Vec<i32> {
    codes
        .iter()
        .map(|c| c.map(|v| v.round() as i32).unwrap_or(0))
        .collect()
}

fn wmo_code_group(code: i32) -> u8 {
    match code {
        0..=3 => 0,
        45 | 48 => 1,
        51..=67 => 2,
        71..=77 => 3,
        80..=86 => 2,
        95..=99 => 4,
        _ => 0,
    }
}

fn wmo_group_name(group: u8) -> &'static str {
    match group {
        0 => "晴好",
        1 => "雾",
        2 => "降水",
        3 => "降雪",
        4 => "雷暴",
        _ => "未知",
    }
}

struct ForecastInfo {
    duration_hours: f64,
    next_condition: Option<String>,
    next_condition_start: Option<String>,
}

fn estimate_forecast(
    current_time: &str,
    current_code: i32,
    hourly_times: &[String],
    hourly_codes: &[i32],
) -> Option<ForecastInfo> {
    use chrono::NaiveDateTime;

    let current_dt = current_time
        .parse::<NaiveDateTime>()
        .or_else(|_| NaiveDateTime::parse_from_str(current_time, "%Y-%m-%dT%H:%M"))
        .ok()?;

    let current_hour = current_dt.format("%Y-%m-%dT%H:00").to_string();
    let current_group = wmo_code_group(current_code);

    let start_idx = hourly_times
        .iter()
        .position(|t| t == &current_hour)
        .unwrap_or_else(|| {
            hourly_times
                .iter()
                .position(|t| t > &current_hour)
                .unwrap_or(hourly_times.len())
        });

    if start_idx >= hourly_codes.len() {
        return None;
    }

    let mut count = 0.0;
    let mut next_group: Option<u8> = None;
    let mut next_idx = None;
    for i in start_idx..hourly_codes.len().min(start_idx + 48) {
        let g = wmo_code_group(hourly_codes[i]);
        if g == current_group {
            count += 1.0;
        } else {
            next_group = Some(g);
            next_idx = Some(i);
            break;
        }
    }

    if count == 0.0 {
        return None;
    }

    let (next_condition, next_condition_start) = match (next_group, next_idx) {
        (Some(g), Some(idx)) if idx < hourly_times.len() => {
            let name = wmo_group_name(g).to_string();
            let start_time = hourly_times[idx]
                .parse::<NaiveDateTime>()
                .or_else(|_| NaiveDateTime::parse_from_str(&hourly_times[idx], "%Y-%m-%dT%H:%M"))
                .map(|dt| dt.format("%m/%d %H:%M").to_string())
                .ok();
            (Some(name), start_time)
        }
        _ => (None, None),
    };

    Some(ForecastInfo {
        duration_hours: count,
        next_condition,
        next_condition_start,
    })
}

impl OpenMeteoProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                eprintln!("警告: 创建自定义 HTTP 客户端失败: {}", e);
                eprintln!("使用默认客户端及标准超时设置。");
                reqwest::Client::new()
            });

        Self {
            client,
            base_url: OPEN_METEO_BASE_URL.to_string(),
        }
    }

    fn temperature_unit_param(unit: &TemperatureUnit) -> &'static str {
        match unit {
            TemperatureUnit::Celsius => "celsius",
            TemperatureUnit::Fahrenheit => "fahrenheit",
        }
    }

    fn wind_speed_unit_param(unit: &WindSpeedUnit) -> &'static str {
        match unit {
            WindSpeedUnit::Kmh => "kmh",
            WindSpeedUnit::Ms => "ms",
            WindSpeedUnit::Mph => "mph",
            WindSpeedUnit::Kn => "kn",
        }
    }

    fn precipitation_unit_param(unit: &PrecipitationUnit) -> &'static str {
        match unit {
            PrecipitationUnit::Mm => "mm",
            PrecipitationUnit::Inch => "inch",
        }
    }

    fn build_url(&self, location: &WeatherLocation, units: &WeatherUnits) -> String {
        format!(
            "{}?latitude={}&longitude={}&current=temperature_2m,is_day,precipitation,weather_code,wind_speed_10m,wind_direction_10m&daily=temperature_2m_max,temperature_2m_min&hourly=weather_code&temperature_unit={}&wind_speed_unit={}&precipitation_unit={}&timezone=auto",
            self.base_url,
            location.latitude,
            location.longitude,
            Self::temperature_unit_param(&units.temperature),
            Self::wind_speed_unit_param(&units.wind_speed),
            Self::precipitation_unit_param(&units.precipitation)
        )
    }
}

impl Default for OpenMeteoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WeatherProvider for OpenMeteoProvider {
    fn get_attribution(&self) -> &'static str {
        ""
    }

    async fn get_current_weather(
        &self,
        location: &WeatherLocation,
        units: &WeatherUnits,
    ) -> Result<WeatherProviderResponse, WeatherError> {
        let url = self.build_url(location, units);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .and_then(|resp| resp.error_for_status())
            .map_err(|e| WeatherError::Network(NetworkError::from_reqwest(e, &url, 30)))?;

        let data: OpenMeteoResponse = response
            .json()
            .await
            .map_err(|e| WeatherError::Network(NetworkError::from_reqwest(e, &url, 30)))?;

        let daily_high = data.daily.as_ref().and_then(|d| {
            d.temperature_2m_max
                .first()
                .map(|t| normalize_temperature(*t, units.temperature))
        });

        let daily_low = data.daily.as_ref().and_then(|d| {
            d.temperature_2m_min
                .first()
                .map(|t| normalize_temperature(*t, units.temperature))
        });

        let forecast = data.hourly.as_ref().and_then(|h| {
            let codes = hourly_codes_to_i32(&h.weather_code);
            estimate_forecast(&data.current.time, data.current.weather_code, &h.time, &codes)
        });

        let condition_duration_hours = forecast.as_ref().map(|f| f.duration_hours);
        let next_condition = forecast.as_ref().and_then(|f| f.next_condition.clone());
        let next_condition_start = forecast.as_ref().and_then(|f| f.next_condition_start.clone());

        Ok(WeatherProviderResponse {
            weather_code: data.current.weather_code,
            temperature: normalize_temperature(data.current.temperature_2m, units.temperature),
            precipitation: normalize_precipitation(data.current.precipitation, units.precipitation),
            wind_speed: normalize_wind_speed(data.current.wind_speed_10m, units.wind_speed),
            wind_direction: data.current.wind_direction_10m,
            sun: CelestialEvents::only_day(data.current.is_day),
            moon_phase: Some(0.5),
            timestamp: data.current.time,
            attribution: self.get_attribution().to_string(),
            daily_high,
            daily_low,
            condition_duration_hours,
            next_condition,
            next_condition_start,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_conversion_params() {
        assert_eq!(
            OpenMeteoProvider::temperature_unit_param(&TemperatureUnit::Celsius),
            "celsius"
        );
        assert_eq!(
            OpenMeteoProvider::temperature_unit_param(&TemperatureUnit::Fahrenheit),
            "fahrenheit"
        );
        assert_eq!(
            OpenMeteoProvider::wind_speed_unit_param(&WindSpeedUnit::Kmh),
            "kmh"
        );
        assert_eq!(
            OpenMeteoProvider::wind_speed_unit_param(&WindSpeedUnit::Ms),
            "ms"
        );
        assert_eq!(
            OpenMeteoProvider::precipitation_unit_param(&PrecipitationUnit::Mm),
            "mm"
        );
    }

    #[test]
    fn test_wmo_code_group() {
        assert_eq!(wmo_code_group(0), 0);
        assert_eq!(wmo_code_group(2), 0);
        assert_eq!(wmo_code_group(45), 1);
        assert_eq!(wmo_code_group(61), 2);
        assert_eq!(wmo_code_group(71), 3);
        assert_eq!(wmo_code_group(80), 2);
        assert_eq!(wmo_code_group(95), 4);
    }

    #[test]
    fn test_estimate_forecast() {
        let times = vec![
            "2024-01-01T12:00".to_string(),
            "2024-01-01T13:00".to_string(),
            "2024-01-01T14:00".to_string(),
            "2024-01-01T15:00".to_string(),
            "2024-01-01T16:00".to_string(),
        ];
        let codes = vec![0, 1, 2, 61, 61];

        let forecast = estimate_forecast("2024-01-01T12:00", 0, &times, &codes).unwrap();
        assert_eq!(forecast.duration_hours, 3.0);
        assert_eq!(forecast.next_condition, Some("降水".to_string()));
        assert_eq!(forecast.next_condition_start, Some("01/01 15:00".to_string()));

        let forecast = estimate_forecast("2024-01-01T15:00", 61, &times, &codes).unwrap();
        assert_eq!(forecast.duration_hours, 2.0);
        assert_eq!(forecast.next_condition, None);
    }
}
