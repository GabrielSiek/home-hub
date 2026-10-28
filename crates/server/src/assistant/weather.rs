//! Weather from Open-Meteo (no key needed), for a named place or wherever the IP address is.

use serde::{Deserialize, Serialize};

use tvl_kit::{Error, Result};

#[derive(Debug, Serialize)]
pub struct Forecast {
    pub place: String,
    pub current: Current,
    pub days: Vec<Day>,
}

#[derive(Debug, Serialize)]
pub struct Current {
    pub temperature_c: f32,
    pub feels_like_c: f32,
    pub humidity_pct: f32,
    pub wind_kmh: f32,
    pub conditions: &'static str,
    #[serde(skip)]
    pub code: u8,
}

#[derive(Debug, Serialize)]
pub struct Day {
    pub date: String,
    pub conditions: &'static str,
    pub min_c: f32,
    pub max_c: f32,
    pub rain_chance_pct: Option<f32>,
}

struct Place {
    name: String,
    lat: f64,
    lon: f64,
}

#[derive(Deserialize)]
struct GeoResults {
    #[serde(default)]
    results: Vec<GeoPlace>,
}

#[derive(Deserialize)]
struct GeoPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    admin1: Option<String>,
    country: Option<String>,
}

#[derive(Deserialize)]
struct IpPlace {
    city: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

#[derive(Deserialize)]
struct IpApi {
    city: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Deserialize)]
struct Response {
    current: RawCurrent,
    daily: RawDaily,
}

#[derive(Deserialize)]
struct RawCurrent {
    temperature_2m: f32,
    apparent_temperature: f32,
    relative_humidity_2m: f32,
    wind_speed_10m: f32,
    weather_code: u8,
}

#[derive(Deserialize)]
struct RawDaily {
    time: Vec<String>,
    weather_code: Vec<u8>,
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f32>>,
}

/// Today and the next `days - 1` days. `place` None: the configured one, else by IP.
pub async fn forecast(http: &reqwest::Client, place: Option<&str>, language: &str, days: u8) -> Result<Forecast> {
    let place = match place.map(str::trim).filter(|p| !p.is_empty()) {
        Some(name) => geocode(http, name, language).await?,
        None => locate(http).await?,
    };
    let resp: Response = http
        .get("https://api.open-meteo.com/v1/forecast")
        .query(&[
            ("latitude", place.lat.to_string()),
            ("longitude", place.lon.to_string()),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code".into(),
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max".into(),
            ),
            ("timezone", "auto".into()),
            ("forecast_days", days.clamp(1, 7).to_string()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let d = resp.daily;
    let days = d
        .time
        .iter()
        .enumerate()
        .map(|(i, date)| Day {
            date: date.clone(),
            conditions: describe(d.weather_code.get(i).copied().unwrap_or(0)),
            min_c: d.temperature_2m_min.get(i).copied().unwrap_or_default(),
            max_c: d.temperature_2m_max.get(i).copied().unwrap_or_default(),
            rain_chance_pct: d.precipitation_probability_max.get(i).copied().flatten(),
        })
        .collect();
    let c = resp.current;
    Ok(Forecast {
        place: place.name,
        current: Current {
            temperature_c: c.temperature_2m,
            feels_like_c: c.apparent_temperature,
            humidity_pct: c.relative_humidity_2m,
            wind_kmh: c.wind_speed_10m,
            conditions: describe(c.weather_code),
            code: c.weather_code,
        },
        days,
    })
}

async fn geocode(http: &reqwest::Client, name: &str, language: &str) -> Result<Place> {
    let lang = language.split('-').next().unwrap_or("en");
    let r: GeoResults = http
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query(&[("name", name), ("count", "1"), ("language", lang), ("format", "json")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let p = r
        .results
        .into_iter()
        .next()
        .ok_or_else(|| Error::msg(format!("No place called '{name}' found")))?;
    let region = p.admin1.or(p.country).filter(|r| *r != p.name);
    Ok(Place {
        name: match region {
            Some(r) => format!("{}, {r}", p.name),
            None => p.name,
        },
        lat: p.latitude,
        lon: p.longitude,
    })
}

/// Where this PC is, from its public IP address. Two free services, as either may rate-limit.
async fn locate(http: &reqwest::Client) -> Result<Place> {
    let first = async {
        let p: IpPlace = http.get("https://ipapi.co/json/").send().await?.error_for_status()?.json().await?;
        Ok::<_, Error>((p.city, p.latitude, p.longitude))
    };
    let found = match first.await {
        Ok(found) => found,
        Err(_) => {
            let p: IpApi = http
                .get("http://ip-api.com/json/?fields=city,lat,lon")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            (p.city, p.lat, p.lon)
        }
    };
    match found {
        (city, Some(lat), Some(lon)) => Ok(Place {
            name: city.unwrap_or_else(|| "your area".into()),
            lat,
            lon,
        }),
        _ => Err(Error::msg("Couldn't tell where you are; set a city for the weather in Settings")),
    }
}

/// WMO weather code, in words.
pub fn describe(code: u8) -> &'static str {
    match code {
        0 => "clear sky",
        1 => "mainly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 | 73 | 75 | 77 => "snow",
        80 | 81 => "rain showers",
        82 => "heavy rain showers",
        85 | 86 => "snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown",
    }
}

/// The icon the UI shows for a weather code.
pub fn icon(code: u8) -> &'static str {
    match code {
        0 | 1 => "sun",
        2 | 3 | 45 | 48 => "cloud",
        _ => "rain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forecast_with_missing_rain_chance() {
        let r: Response = serde_json::from_str(
            r#"{"current":{"temperature_2m":24.1,"apparent_temperature":25.0,"relative_humidity_2m":60,
                "wind_speed_10m":8.2,"weather_code":2},
               "daily":{"time":["2026-10-22","2026-10-23"],"weather_code":[2,61],
                "temperature_2m_max":[28.0,22.5],"temperature_2m_min":[18.0,17.1],
                "precipitation_probability_max":[10,null]}}"#,
        )
        .unwrap();
        assert_eq!(describe(r.current.weather_code), "partly cloudy");
        assert_eq!(r.daily.precipitation_probability_max, vec![Some(10.0), None]);
        assert_eq!(icon(61), "rain");
    }

    /// Calls Open-Meteo: `cargo test -- --ignored real_weather`.
    #[test]
    #[ignore]
    fn real_weather() {
        let http = reqwest::Client::new();
        let f = tokio::runtime::Runtime::new().unwrap().block_on(forecast(&http, Some("São Paulo"), "pt-BR", 2)).unwrap();
        assert_eq!(f.days.len(), 2);
        println!("{f:?}");
        let here = tokio::runtime::Runtime::new().unwrap().block_on(forecast(&http, None, "pt-BR", 1)).unwrap();
        println!("by IP: {}", here.place);
    }
}
