//! The drive from home to an event's place, on OpenStreetMap's free services: Nominatim finds
//! the two places, OSRM's public server the route (by car, without traffic). Nominatim asks for
//! one request a second at most and a name for the app; each place is asked about once and kept
//! in calendar-places.json, found or not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use tvl_kit::Result;
use tvl_kit::time::now_ms;
use tvl_kit::text::words;

const FILE: &str = "calendar-places.json";
const SEARCH: &str = "https://nominatim.openstreetmap.org/search";
const ROUTE: &str = "https://router.project-osrm.org/route/v1/driving";
const UA: &str = "tv-launcher/0.1 (the calendar of a TV launcher)";
/// Further than this, it isn't a drive.
const MAX_KM: f64 = 400.0;
/// How far around home (in degrees) a place is looked for first.
const NEAR: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Travel {
    pub km: f64,
    pub minutes: i64,
}

/// Latitude and longitude.
type Spot = (f64, f64);

pub struct Places {
    path: PathBuf,
    /// By what was looked for, folded; None when Nominatim didn't find it.
    found: Mutex<HashMap<String, Option<Spot>>>,
    /// Drives by home and place, folded, while the launcher runs.
    drives: Mutex<HashMap<(String, String), Option<Travel>>>,
    /// When Nominatim was last asked; one request at a time.
    asked: tokio::sync::Mutex<i64>,
}

impl Places {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let found = tvl_kit::store::load(&path).unwrap_or_default();
        Self {
            path,
            found: Mutex::new(found),
            drives: Mutex::default(),
            asked: tokio::sync::Mutex::new(0),
        }
    }
}

/// The drive from `home` to `place`: None when either isn't found, the place is a link (a call),
/// or it's too far to drive.
pub async fn drive(http: &reqwest::Client, places: &Places, home: &str, place: &str) -> Result<Option<Travel>> {
    if place.contains("://") {
        return Ok(None);
    }
    let key = (words(home), words(place));
    if let Some(t) = places.drives.lock().get(&key) {
        return Ok(*t);
    }
    let Some(from) = find(http, places, home, None).await? else {
        return Ok(None);
    };
    let mut to = None;
    for q in candidates(place) {
        to = find(http, places, &q, Some(from)).await?;
        if to.is_some() {
            break;
        }
    }
    let travel = match to {
        Some(to) => {
            let url = format!("{ROUTE}/{},{};{},{}?overview=false", from.1, from.0, to.1, to.0);
            let v: Value = http.get(url).header("User-Agent", UA).send().await?.error_for_status()?.json().await?;
            parse_route(&v)
        }
        None => None,
    };
    places.drives.lock().insert(key, travel);
    Ok(travel)
}

/// What to look for: the whole place, then without its first part, often a name Nominatim
/// doesn't know ("Clínica Sorriso, Av. Ipiranga, 1234 - Azenha, Porto Alegre - RS"). Brazilian
/// addresses separate parts with " - " too.
fn candidates(place: &str) -> Vec<String> {
    let place = place.replace(" - ", ", ");
    let parts: Vec<&str> = place.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut out = vec![parts.join(", ")];
    if parts.len() >= 3 {
        out.push(parts[1..].join(", "));
    }
    out
}

/// Where `q` is, preferring places around `near`.
async fn find(http: &reqwest::Client, places: &Places, q: &str, near: Option<Spot>) -> Result<Option<Spot>> {
    let key = words(q);
    if let Some(s) = places.found.lock().get(&key) {
        return Ok(*s);
    }
    let mut last = places.asked.lock().await;
    let wait = *last + 1100 - now_ms();
    if wait > 0 {
        tokio::time::sleep(Duration::from_millis(wait as u64)).await;
    }
    let mut query = vec![("q", q.to_string()), ("format", "jsonv2".into()), ("limit", "1".into())];
    if let Some((lat, lon)) = near {
        query.push(("viewbox", format!("{},{},{},{}", lon - NEAR, lat + NEAR, lon + NEAR, lat - NEAR)));
    }
    let r = http.get(SEARCH).query(&query).header("User-Agent", UA).send().await;
    *last = now_ms();
    let v: Value = r?.error_for_status()?.json().await?;
    let spot = parse_place(&v);
    let mut found = places.found.lock();
    found.insert(key, spot);
    if let Err(e) = tvl_kit::store::save(&places.path, &*found, false) {
        tracing::warn!("can't keep the places found: {e}");
    }
    Ok(spot)
}

fn parse_place(v: &Value) -> Option<Spot> {
    let n = |k: &str| v[0][k].as_str()?.parse::<f64>().ok();
    Some((n("lat")?, n("lon")?))
}

fn parse_route(v: &Value) -> Option<Travel> {
    if v["code"].as_str() != Some("Ok") {
        return None;
    }
    let r = &v["routes"][0];
    let km = r["distance"].as_f64()? / 1000.0;
    let minutes = (r["duration"].as_f64()? / 60.0).ceil() as i64;
    (km <= MAX_KM).then_some(Travel { km: (km * 10.0).round() / 10.0, minutes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_to_look_for() {
        assert_eq!(
            candidates("Clínica Sorriso, Av. Ipiranga, 1234 - Azenha, Porto Alegre - RS, 90160-091, Brasil"),
            [
                "Clínica Sorriso, Av. Ipiranga, 1234, Azenha, Porto Alegre, RS, 90160-091, Brasil",
                "Av. Ipiranga, 1234, Azenha, Porto Alegre, RS, 90160-091, Brasil",
            ]
        );
        assert_eq!(candidates("Arena do Grêmio"), ["Arena do Grêmio"]);
    }

    #[test]
    fn answers() {
        let place: Value = serde_json::from_str(r#"[{"lat":"-30.0277","lon":"-51.2287","display_name":"Porto Alegre"}]"#).unwrap();
        assert_eq!(parse_place(&place), Some((-30.0277, -51.2287)));
        assert_eq!(parse_place(&Value::Array(vec![])), None);

        let route: Value =
            serde_json::from_str(r#"{"code":"Ok","routes":[{"distance":4213.7,"duration":1471.2}]}"#).unwrap();
        assert_eq!(parse_route(&route), Some(Travel { km: 4.2, minutes: 25 }));
        let far: Value = serde_json::from_str(r#"{"code":"Ok","routes":[{"distance":1100000,"duration":40000}]}"#).unwrap();
        assert_eq!(parse_route(&far), None);
        assert_eq!(parse_route(&serde_json::json!({"code":"NoRoute"})), None);
    }
}
