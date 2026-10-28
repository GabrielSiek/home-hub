//! ESPN's public site API: scoreboards, game summaries and standings, no key needed.
//! Soccer scoreboards take one `dates=` day at a time (ranges answer 400).

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::dates::Day;
use super::{Game, League, Phase, Sport, Team};
use crate::photos::rfc3339_ms;

const SITE: &str = "https://site.api.espn.com/apis/site/v2/sports";
const STANDINGS: &str = "https://site.api.espn.com/apis/v2/sports";

fn path(league: &League) -> String {
    match league.sport {
        Sport::Basketball => format!("basketball/{}", league.id),
        Sport::Soccer => format!("soccer/{}", league.id),
    }
}

pub fn scoreboard_url(league: &League, day: Option<Day>) -> String {
    match day {
        Some(d) => format!("{SITE}/{}/scoreboard?dates={}&limit=100", path(league), d.compact()),
        None => format!("{SITE}/{}/scoreboard", path(league)),
    }
}

pub fn summary_url(league: &League, id: &str) -> String {
    format!("{SITE}/{}/summary?event={id}", path(league))
}

pub fn standings_url(league: &League) -> String {
    format!("{STANDINGS}/{}/standings", path(league))
}

pub fn schedule_url(league: &League, team: &str, fixtures: bool) -> String {
    let fixture = if fixtures { "?fixture=true" } else { "" };
    format!("{SITE}/{}/teams/{team}/schedule{fixture}", path(league))
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// "0093EC" → "#0093EC"; ESPN leaves some teams without a color.
fn color(v: &Value) -> String {
    match v.as_str() {
        Some(c) if c.len() == 6 && c != "000000" && c != "ffffff" => format!("#{c}"),
        _ => "#2a2a2e".to_string(),
    }
}

/// A score as the scoreboard ("2") or a schedule ({ "displayValue": "2" }) writes it.
fn score(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(_) => v["displayValue"].as_str().map(str::to_string),
        _ => None,
    }
}

pub fn team(c: &Value) -> Team {
    let t = if c["team"].is_object() { &c["team"] } else { c };
    Team {
        id: s(&t["id"]),
        name: t["shortDisplayName"]
            .as_str()
            .or(t["displayName"].as_str())
            .or(t["name"].as_str())
            .unwrap_or_default()
            .to_string(),
        abbr: s(&t["abbreviation"]),
        logo: t["logo"]
            .as_str()
            .or(t["logos"][0]["href"].as_str())
            .map(str::to_string),
        color: color(&t["color"]),
        score: score(&c["score"]),
        winner: c["winner"].as_bool().unwrap_or(false),
    }
}

/// "Q3", "OT", "2OT".
pub fn period_name(p: i64) -> String {
    match p {
        ..=4 => format!("Q{}", p.max(1)),
        5 => "OT".to_string(),
        _ => format!("{}OT", p - 4),
    }
}

/// Seconds from "7:46" or "45.2".
fn clock_secs(text: &str) -> f64 {
    match text.split_once(':') {
        Some((m, s)) => m.parse::<f64>().unwrap_or(0.0) * 60.0 + s.parse::<f64>().unwrap_or(0.0),
        None => text.parse().unwrap_or(0.0),
    }
}

/// The minute in "67'" or "45'+3'".
fn minute(text: &str) -> f64 {
    text.split(['\'', '+']).next().and_then(|m| m.trim().parse().ok()).unwrap_or(0.0)
}

/// Phase, what to show for it, how far through, and a note for a postponed game.
fn status(sport: Sport, st: &Value) -> (Phase, String, f32, Option<String>) {
    let ty = &st["type"];
    let name = ty["name"].as_str().unwrap_or_default();
    let short = ty["shortDetail"].as_str().unwrap_or_default();
    let period = st["period"].as_i64().unwrap_or(0);
    let clock = st["displayClock"].as_str().unwrap_or_default();
    for (flag, note) in [("POSTPONED", "Postponed"), ("CANCELED", "Canceled"), ("SUSPENDED", "Suspended"), ("DELAYED", "Delayed")] {
        if name.contains(flag) {
            return (Phase::Pre, String::new(), 0.0, Some(note.to_string()));
        }
    }
    match ty["state"].as_str() {
        Some("in") => match sport {
            Sport::Basketball => {
                let text = if name.contains("HALFTIME") {
                    "Halftime".to_string()
                } else if name.contains("END_PERIOD") {
                    format!("End of {}", period_name(period))
                } else {
                    format!("{} · {clock}", period_name(period))
                };
                let played = if period <= 4 {
                    (period - 1) as f64 * 720.0 + (720.0 - clock_secs(clock))
                } else {
                    2880.0
                };
                (Phase::Live, text, (played / 2880.0).clamp(0.0, 1.0) as f32, None)
            }
            Sport::Soccer => {
                let text = if name.contains("HALFTIME") { "HT".to_string() } else { clock.to_string() };
                let m = if name.contains("HALFTIME") { 45.0 } else { minute(clock) };
                (Phase::Live, text, (m / 90.0).clamp(0.0, 1.0) as f32, None)
            }
        },
        Some("post") => {
            let text = match sport {
                Sport::Basketball if period > 4 => format!("Final/{}", period_name(period)),
                Sport::Basketball => "Final".to_string(),
                Sport::Soccer => match short {
                    "FT-Pens" => "Penalties".to_string(),
                    "AET" => "Extra time".to_string(),
                    _ => "Full time".to_string(),
                },
            };
            (Phase::Post, text, 1.0, None)
        }
        _ => (Phase::Pre, String::new(), 0.0, None),
    }
}

fn pretty_slug(slug: &str) -> String {
    let mut s = slug.replace('-', " ");
    if let Some(first) = s.get(..1) {
        s = first.to_uppercase() + &s[1..];
    }
    s
}

/// The two-leg tie an event belongs to: the stage, the leg, and each side's aggregate.
pub struct TieInfo {
    pub title: String,
    pub leg: Option<i64>,
    pub aggregate: HashMap<String, i64>,
}

pub fn tie_info(e: &Value) -> Option<TieInfo> {
    let c = &e["competitions"][0];
    let series = &c["series"];
    let title = series["title"].as_str()?.to_string();
    let aggregate = series["competitors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| Some((s(&x["id"]), x["aggregateScore"].as_f64()? as i64)))
        .collect();
    Some(TieInfo { title, leg: c["leg"]["value"].as_i64(), aggregate })
}

/// One scoreboard (or team schedule) event.
pub fn parse_event(league: &League, e: &Value) -> Option<Game> {
    let c = &e["competitions"][0];
    let comps = c["competitors"].as_array()?;
    let side = |ha: &str| comps.iter().find(|x| x["homeAway"] == ha).map(team);
    let (home, away) = (side("home")?, side("away")?);
    let st = if e["status"].is_object() { &e["status"] } else { &c["status"] };
    let (phase, status_text, progress, note) = status(league.sport, st);
    let (home, away) = if phase == Phase::Pre {
        (Team { score: None, ..home }, Team { score: None, ..away })
    } else {
        (home, away)
    };
    let headline = c["notes"][0]["headline"].as_str().unwrap_or_default();
    let stage = match league.sport {
        Sport::Soccer => {
            let parts: Vec<&str> = [c["series"]["title"].as_str().unwrap_or_default(), headline]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect();
            (!parts.is_empty()).then(|| parts.join(" · "))
        }
        Sport::Basketball => {
            if headline.is_empty() {
                e["season"]["slug"].as_str().map(pretty_slug)
            } else {
                Some(headline.to_string())
            }
        }
    };
    let aggregate = tie_info(e).filter(|t| t.leg == Some(2) && phase != Phase::Pre).and_then(|t| {
        Some(format!("Agg {}–{}", t.aggregate.get(&home.id)?, t.aggregate.get(&away.id)?))
    });
    Some(Game {
        id: s(&e["id"]),
        league: league.id.to_string(),
        league_name: league.name.to_string(),
        sport: league.sport,
        start: rfc3339_ms(e["date"].as_str()?)?,
        phase,
        status: status_text,
        progress,
        home,
        away,
        venue: c["venue"]["fullName"].as_str().map(str::to_string),
        stage,
        channels: Vec::new(),
        mine: None,
        aggregate,
        ge_video: None,
        note,
    })
}

pub fn parse_events(league: &League, v: &Value) -> Vec<Game> {
    v["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| parse_event(league, e))
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Stage {
    pub label: String,
    /// "Nov 1-8".
    pub detail: String,
    pub current: bool,
}

/// A cup's stages from the scoreboard's calendar.
pub fn stages(v: &Value, now: i64) -> Vec<Stage> {
    let entries = v["leagues"][0]["calendar"][0]["entries"].as_array();
    entries
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let start = rfc3339_ms(e["startDate"].as_str()?)?;
            let end = rfc3339_ms(e["endDate"].as_str()?)?;
            Some(Stage {
                label: s(&e["label"]),
                detail: s(&e["detail"]),
                current: (start..end).contains(&now),
            })
        })
        .collect()
}

// ---------- Game details ----------

#[derive(Debug, Clone, Serialize)]
pub struct Stat {
    pub name: String,
    pub home: String,
    pub away: String,
    /// For the bars; 0 when not a number.
    pub home_v: f64,
    pub away_v: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchEvent {
    /// 0–90+, for the timeline.
    pub minute: f64,
    /// "45'+3'".
    pub clock: String,
    /// goal, yellow, red, sub.
    pub kind: &'static str,
    pub home: bool,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Player {
    pub n: String,
    pub name: String,
    /// On the home team's half, its own goal at 0: depth 0..0.5 and width 0..1.
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Lineup {
    pub formation: String,
    pub players: Vec<Player>,
    /// Who came on.
    pub subs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoccerDetail {
    /// Most recent first.
    pub events: Vec<MatchEvent>,
    pub home_scorers: String,
    pub away_scorers: String,
    pub stats: Vec<Stat>,
    pub home_lineup: Lineup,
    pub away_lineup: Lineup,
    /// Attack momentum by 3-minute spell from the kick-off, up to now: −1 (the away team
    /// pressing) to 1 (the home team), from the commentary's shots and corners.
    pub momentum: Vec<f64>,
    /// Both teams in the league table (home first), when it's a league.
    pub table: Vec<Spot>,
}

/// A team's place in the table, and how many places this result moved it (up is positive).
#[derive(Debug, Clone, Serialize)]
pub struct Spot {
    pub team: String,
    pub rank: i64,
    pub points: i64,
    pub moved: i64,
}

/// A field goal on the court: `x` along it, 0 at the home team's basket end (where it shoots)
/// and 1 at the other, `y` across it, both 0..1.
#[derive(Debug, Clone, Serialize)]
pub struct Shot {
    pub home: bool,
    pub x: f64,
    pub y: f64,
    pub made: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Leader {
    pub name: String,
    pub team: String,
    pub color: String,
    pub category: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoxRow {
    pub name: String,
    pub pos: String,
    pub starter: bool,
    pub min: String,
    pub pts: String,
    pub reb: String,
    pub ast: String,
    pub fg: String,
    pub tp: String,
    pub ft: String,
    pub stl: String,
    pub blk: String,
    pub pm: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Play {
    pub clock: String,
    pub team: Option<String>,
    pub text: String,
    /// Away–home.
    pub score: String,
    pub scoring: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NbaDetail {
    pub periods: Vec<String>,
    pub away_line: Vec<String>,
    pub home_line: Vec<String>,
    pub stats: Vec<Stat>,
    /// (minute 0–48+, the away team's chance to win 0–100), oldest first.
    pub win_prob: Vec<(f64, f64)>,
    pub leaders: Vec<Leader>,
    pub away_box: Vec<BoxRow>,
    pub home_box: Vec<BoxRow>,
    /// Most recent first.
    pub plays: Vec<Play>,
    pub info: Vec<String>,
    pub shots: Vec<Shot>,
    /// Biggest lead, lead changes, the last minutes' run or the season series.
    pub facts: Vec<Fact>,
}

fn by_team<'a>(list: &'a Value, id: &str) -> Option<&'a Value> {
    list.as_array()?.iter().find(|x| x["team"]["id"].as_str() == Some(id))
}

fn number(text: &str) -> f64 {
    text.trim_end_matches('%').parse().unwrap_or(0.0)
}

pub fn parse_nba(v: &Value, game: &Game) -> NbaDetail {
    let comps = &v["header"]["competitions"][0]["competitors"];
    let line = |id: &str| -> Vec<String> {
        by_team_header(comps, id)
            .and_then(|c| c["linescores"].as_array())
            .map(|ls| {
                ls.iter()
                    .map(|l| l["displayValue"].as_str().map(str::to_string).unwrap_or_else(|| s(&l["value"])))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (mut away_line, mut home_line) = (line(&game.away.id), line(&game.home.id));
    let count = away_line.len().max(home_line.len()).max(4);
    for l in [&mut away_line, &mut home_line] {
        l.resize(count, "–".to_string());
    }
    let periods = (1..=count as i64).map(period_name).collect();

    let teams = &v["boxscore"]["teams"];
    let team_stat = |id: &str, name: &str| -> String {
        by_team(teams, id)
            .and_then(|t| t["statistics"].as_array())
            .and_then(|st| st.iter().find(|x| x["name"] == name))
            .map(|x| s(&x["displayValue"]))
            .unwrap_or_default()
    };
    let stats = [
        ("Field goals", "fieldGoalsMade-fieldGoalsAttempted"),
        ("FG %", "fieldGoalPct"),
        ("3-pointers", "threePointFieldGoalsMade-threePointFieldGoalsAttempted"),
        ("Free throws", "freeThrowsMade-freeThrowsAttempted"),
        ("Rebounds", "totalRebounds"),
        ("Assists", "assists"),
        ("Turnovers", "turnovers"),
        ("Steals", "steals"),
        ("Blocks", "blocks"),
    ]
    .into_iter()
    .filter_map(|(label, key)| {
        let (home, away) = (team_stat(&game.home.id, key), team_stat(&game.away.id, key));
        (!home.is_empty() || !away.is_empty()).then(|| Stat {
            name: label.to_string(),
            home_v: number(&home),
            away_v: number(&away),
            home,
            away,
        })
    })
    .collect();

    // Win probability by play; the plays say when each happened.
    let plays_v = v["plays"].as_array().cloned().unwrap_or_default();
    let when: HashMap<String, f64> = plays_v
        .iter()
        .filter_map(|p| {
            let period = p["period"]["number"].as_i64()?;
            let left = clock_secs(p["clock"]["displayValue"].as_str()?);
            let t = if period <= 4 {
                (period - 1) as f64 * 12.0 + 12.0 - left / 60.0
            } else {
                48.0 + (period - 5) as f64 * 5.0 + 5.0 - left / 60.0
            };
            Some((s(&p["id"]), t))
        })
        .collect();
    let probs: Vec<(f64, f64)> = v["winprobability"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let t = *when.get(w["playId"].as_str()?)?;
            Some((t, (1.0 - w["homeWinPercentage"].as_f64()?) * 100.0))
        })
        .collect();
    let step = (probs.len() / 120).max(1);
    let mut win_prob: Vec<(f64, f64)> = probs.iter().step_by(step).copied().collect();
    if let (Some(last), Some(kept)) = (probs.last(), win_prob.last()) {
        if last != kept {
            win_prob.push(*last);
        }
    }

    let mut leaders = Vec::new();
    for cat in ["points", "rebounds", "assists"] {
        for t in [&game.away, &game.home] {
            let Some(group) = by_team(&v["leaders"], &t.id) else { continue };
            let Some(c) = group["leaders"].as_array().and_then(|l| l.iter().find(|x| x["name"] == cat)) else {
                continue;
            };
            let top = &c["leaders"][0];
            if top.is_null() {
                continue;
            }
            leaders.push(Leader {
                name: s(&top["athlete"]["displayName"]),
                team: t.abbr.clone(),
                color: t.color.clone(),
                category: s(&c["displayName"]),
                value: s(&top["displayValue"]),
            });
        }
    }

    let box_for = |id: &str| -> Vec<BoxRow> {
        let Some(p) = by_team(&v["boxscore"]["players"], id) else {
            return Vec::new();
        };
        let st = &p["statistics"][0];
        let keys: Vec<&str> = st["keys"].as_array().into_iter().flatten().filter_map(|k| k.as_str()).collect();
        st["athletes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| !a["didNotPlay"].as_bool().unwrap_or(false))
            .map(|a| {
                let stat = |key: &str| {
                    keys.iter()
                        .position(|k| *k == key)
                        .and_then(|i| a["stats"][i].as_str())
                        .unwrap_or("–")
                        .to_string()
                };
                BoxRow {
                    name: s(&a["athlete"]["displayName"]),
                    pos: s(&a["athlete"]["position"]["abbreviation"]),
                    starter: a["starter"].as_bool().unwrap_or(false),
                    min: stat("minutes"),
                    pts: stat("points"),
                    reb: stat("rebounds"),
                    ast: stat("assists"),
                    fg: stat("fieldGoalsMade-fieldGoalsAttempted"),
                    tp: stat("threePointFieldGoalsMade-threePointFieldGoalsAttempted"),
                    ft: stat("freeThrowsMade-freeThrowsAttempted"),
                    stl: stat("steals"),
                    blk: stat("blocks"),
                    pm: stat("plusMinus"),
                }
            })
            .collect()
    };

    let abbr_of = |id: &str| -> Option<String> {
        [&game.home, &game.away].into_iter().find(|t| t.id == id).map(|t| t.abbr.clone())
    };
    let plays = plays_v
        .iter()
        .rev()
        .take(30)
        .map(|p| Play {
            clock: format!(
                "{} {}",
                period_name(p["period"]["number"].as_i64().unwrap_or(1)),
                p["clock"]["displayValue"].as_str().unwrap_or_default()
            ),
            team: p["team"]["id"].as_str().and_then(abbr_of),
            text: s(&p["text"]),
            score: format!("{}–{}", p["awayScore"].as_i64().unwrap_or(0), p["homeScore"].as_i64().unwrap_or(0)),
            scoring: p["scoringPlay"].as_bool().unwrap_or(false),
        })
        .collect();

    let mut info = Vec::new();
    let series = &v["seasonseries"][0];
    let summary = series["summary"].as_str().filter(|x| !x.is_empty());
    if let Some(sum) = summary.or(series["seriesScore"].as_str().filter(|x| *x != "0-0")) {
        info.push(format!("Season series · {sum}"));
    }
    if let Some(a) = v["gameInfo"]["attendance"].as_i64().filter(|a| *a > 0) {
        info.push(format!("Attendance {}", thousands(a)));
    }
    if let Some(venue) = v["gameInfo"]["venue"]["fullName"].as_str() {
        let city = v["gameInfo"]["venue"]["address"]["city"].as_str();
        info.push(city.map_or_else(|| venue.to_string(), |c| format!("{venue}, {c}")));
    }
    let injuries: Vec<String> = v["injuries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some(format!("{} {}", t["team"]["abbreviation"].as_str()?, t["injuries"].as_array()?.len())))
        .collect();
    if !injuries.is_empty() {
        info.push(format!("Injuries · {}", injuries.join(" · ")));
    }

    // Where each field goal was taken from: ESPN gives feet, across the court (0–50) and out
    // from the basket the team attacks. Free throws have no spot.
    let shots = plays_v
        .iter()
        .filter(|p| p["shootingPlay"].as_bool().unwrap_or(false) && p["pointsAttempted"].as_i64().unwrap_or(0) >= 2)
        .filter_map(|p| {
            let (cx, cy) = (p["coordinate"]["x"].as_f64()?, p["coordinate"]["y"].as_f64()?);
            if !(-5.0..=55.0).contains(&cx) || !(-10.0..=90.0).contains(&cy) {
                return None;
            }
            let home = p["team"]["id"].as_str() == Some(game.home.id.as_str());
            let along = (5.25 + cy) / 94.0;
            let (x, y) = if home { (along, cx / 50.0) } else { (1.0 - along, 1.0 - cx / 50.0) };
            Some(Shot {
                home,
                x: x.clamp(0.01, 0.99),
                y: y.clamp(0.02, 0.98),
                made: p["scoringPlay"].as_bool().unwrap_or(false),
            })
        })
        .collect();

    NbaDetail {
        periods,
        away_line,
        home_line,
        stats,
        win_prob,
        leaders,
        away_box: box_for(&game.away.id),
        home_box: box_for(&game.home.id),
        plays,
        info,
        shots,
        facts: nba_facts(&plays_v, &when, game, summary),
    }
}

/// The score after each play, (minute, away, home), oldest first.
fn score_line(plays: &[Value], when: &HashMap<String, f64>) -> Vec<(f64, i64, i64)> {
    plays
        .iter()
        .filter_map(|p| {
            let t = *when.get(p["id"].as_str()?)?;
            Some((t, p["awayScore"].as_i64()?, p["homeScore"].as_i64()?))
        })
        .collect()
}

fn nba_facts(plays: &[Value], when: &HashMap<String, f64>, game: &Game, series: Option<&str>) -> Vec<Fact> {
    let line = score_line(plays, when);
    let mut facts = Vec::new();
    let (mut away_lead, mut home_lead, mut changes, mut leader) = (0, 0, 0, 0i64);
    for &(_, a, h) in &line {
        away_lead = away_lead.max(a - h);
        home_lead = home_lead.max(h - a);
        let now = (a - h).signum();
        if now != 0 {
            if leader != 0 && now != leader {
                changes += 1;
            }
            leader = now;
        }
    }
    if away_lead > 0 || home_lead > 0 {
        let (t, n) = if away_lead >= home_lead { (&game.away, away_lead) } else { (&game.home, home_lead) };
        facts.push(Fact { label: "Biggest lead".into(), value: format!("{} {n}", t.abbr) });
    }
    if !line.is_empty() {
        facts.push(Fact { label: "Lead changes".into(), value: changes.to_string() });
    }
    match game.phase {
        Phase::Live => {
            // Who scored what in the last three minutes played.
            if let Some(&(end, a1, h1)) = line.last() {
                let (_, a0, h0) = line.iter().find(|(t, _, _)| *t >= end - 3.0).copied().unwrap_or((0.0, 0, 0));
                let (da, dh) = (a1 - a0, h1 - h0);
                if da + dh > 0 {
                    let (t, x, y) = if da >= dh { (&game.away, da, dh) } else { (&game.home, dh, da) };
                    facts.push(Fact { label: "Last 3 min".into(), value: format!("{} {x}–{y} run", t.abbr) });
                }
            }
        }
        _ => {
            // "CHA leads series 1-0" → "CHA 1–0".
            if let Some(s) = series {
                let first = s.split_whitespace().next().unwrap_or_default();
                let score = s.split_whitespace().last().unwrap_or_default().replace('-', "–");
                let value = if first.eq_ignore_ascii_case("series") { format!("Tied {score}") } else { format!("{first} {score}") };
                facts.push(Fact { label: "Series".into(), value });
            }
        }
    }
    facts
}

fn by_team_header<'a>(comps: &'a Value, id: &str) -> Option<&'a Value> {
    comps.as_array()?.iter().find(|c| c["id"].as_str() == Some(id) || c["team"]["id"].as_str() == Some(id))
}

fn thousands(n: i64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn parse_soccer(v: &Value, game: &Game) -> SoccerDetail {
    let mut events = Vec::new();
    let mut scorers: [Vec<String>; 2] = [Vec::new(), Vec::new()];
    for e in v["keyEvents"].as_array().into_iter().flatten() {
        let ty = e["type"]["type"].as_str().unwrap_or_default();
        let kind = match ty {
            _ if ty.starts_with("goal") || ty == "penalty---scored" || ty == "own-goal" => "goal",
            "yellow-card" => "yellow",
            "red-card" | "yellow-red-card" => "red",
            "substitution" => "sub",
            _ => continue,
        };
        let home = e["team"]["id"].as_str() == Some(game.home.id.as_str());
        let names: Vec<String> = e["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| s(&p["athlete"]["displayName"]))
            .collect();
        let first = names.first().cloned().unwrap_or_default();
        let clock = e["clock"]["displayValue"].as_str().unwrap_or_default().to_string();
        let minute = e["clock"]["value"].as_f64().map(|secs| secs / 60.0).unwrap_or_else(|| minute(&clock));
        let (title, detail) = match kind {
            "goal" => {
                let how = match ty {
                    "penalty---scored" => " (pen)",
                    "own-goal" => " (own goal)",
                    _ => "",
                };
                scorers[usize::from(!home)].push(format!("{first} {clock}{how}"));
                let assist = names.get(1).filter(|_| how.is_empty()).map(|a| format!("Assist · {a}"));
                (format!("Goal · {first}{how}"), assist.unwrap_or_default())
            }
            "yellow" => (format!("Yellow card · {first}"), String::new()),
            "red" => (format!("Red card · {first}"), String::new()),
            _ => (
                format!("{first} on"),
                names.get(1).map(|o| format!("{o} off")).unwrap_or_default(),
            ),
        };
        events.push(MatchEvent { minute, clock, kind, home, title, detail });
    }
    events.reverse();

    let teams = &v["boxscore"]["teams"];
    let stat = |id: &str, key: &str| -> Option<f64> {
        by_team(teams, id)?["statistics"]
            .as_array()?
            .iter()
            .find(|x| x["name"] == key)?["displayValue"]
            .as_str()?
            .parse()
            .ok()
    };
    let list: [(&str, &str, f64, &str); 14] = [
        ("Possession", "possessionPct", 1.0, "%"),
        ("Shots", "totalShots", 1.0, ""),
        ("Shots on target", "shotsOnTarget", 1.0, ""),
        ("Corners", "wonCorners", 1.0, ""),
        ("Fouls", "foulsCommitted", 1.0, ""),
        ("Offsides", "offsides", 1.0, ""),
        ("Saves", "saves", 1.0, ""),
        ("Passes", "totalPasses", 1.0, ""),
        ("Pass accuracy", "passPct", 100.0, "%"),
        ("Crosses", "totalCrosses", 1.0, ""),
        ("Tackles", "totalTackles", 1.0, ""),
        ("Interceptions", "interceptions", 1.0, ""),
        ("Yellow cards", "yellowCards", 1.0, ""),
        ("Red cards", "redCards", 1.0, ""),
    ];
    let stats = list
        .into_iter()
        .filter_map(|(name, key, scale, unit)| {
            let (h, a) = (stat(&game.home.id, key), stat(&game.away.id, key));
            if h.is_none() && a.is_none() {
                return None;
            }
            let (h, a) = (h.unwrap_or(0.0) * scale, a.unwrap_or(0.0) * scale);
            let text = |x: f64| format!("{}{unit}", (x * 10.0).round() / 10.0);
            Some(Stat { name: name.to_string(), home: text(h), away: text(a), home_v: h, away_v: a })
        })
        .collect();

    let lineup = |id: &str| -> Lineup {
        let Some(r) = v["rosters"].as_array().and_then(|rs| rs.iter().find(|r| r["team"]["id"].as_str() == Some(id)))
        else {
            return Lineup::default();
        };
        let roster: Vec<&Value> = r["roster"].as_array().into_iter().flatten().collect();
        let starters: Vec<(String, String, String)> = roster
            .iter()
            .filter(|p| p["starter"].as_bool().unwrap_or(false))
            .map(|p| {
                (
                    s(&p["jersey"]),
                    p["athlete"]["shortName"].as_str().map(str::to_string).unwrap_or_else(|| s(&p["athlete"]["displayName"])),
                    s(&p["position"]["abbreviation"]),
                )
            })
            .collect();
        let formation = s(&r["formation"]);
        Lineup {
            players: place(&formation, &starters),
            formation,
            subs: roster
                .iter()
                .filter(|p| p["subbedIn"].as_bool().unwrap_or(false))
                .map(|p| s(&p["athlete"]["displayName"]))
                .collect(),
        }
    };

    SoccerDetail {
        events,
        home_scorers: scorers[0].join(" · "),
        away_scorers: scorers[1].join(" · "),
        stats,
        home_lineup: lineup(&game.home.id),
        away_lineup: lineup(&game.away.id),
        momentum: momentum(v, game),
        table: spots(v, game),
    }
}

/// Spells of three minutes, to the end of added time.
const SPELLS: usize = 32;

/// How much each kind of moment says a team is on top.
fn pressure(kind: &str) -> f64 {
    match kind {
        "Shot On Target" | "Shot Hit Woodwork" => 3.0,
        "Shot Off Target" => 2.0,
        "Shot Blocked" => 1.5,
        "Corner Awarded" => 1.0,
        "Offside" => 0.5,
        _ if kind.starts_with("Goal") || kind.starts_with("Penalty - Scored") => 4.0,
        _ => 0.0,
    }
}

/// Attack momentum from the commentary: the home team's pressure minus the away team's in each
/// spell, scaled to −1..1 (a quiet match doesn't fill the chart).
fn momentum(v: &Value, game: &Game) -> Vec<f64> {
    let mut spells = vec![0.0f64; SPELLS];
    let mut last = 0usize;
    for c in v["commentary"].as_array().into_iter().flatten() {
        let p = &c["play"];
        let w = pressure(p["type"]["text"].as_str().unwrap_or_default());
        let Some(secs) = c["time"]["value"].as_f64() else { continue };
        let i = ((secs / 180.0) as usize).min(SPELLS - 1);
        last = last.max(i + 1);
        let team = p["team"]["displayName"].as_str().unwrap_or_default();
        if w == 0.0 || team.is_empty() {
            continue;
        }
        if super::same_team(team, &game.home.name) {
            spells[i] += w;
        } else if super::same_team(team, &game.away.name) {
            spells[i] -= w;
        }
    }
    let scale = spells.iter().fold(6.0f64, |m, x| m.max(x.abs()));
    spells.truncate(last);
    spells.iter().map(|x| (x / scale * 100.0).round() / 100.0).collect()
}

/// Both teams in the summary's league table, and how many places the result moved them: the
/// table without it, ranked by points, wins and goal difference, against the table with it.
fn spots(v: &Value, game: &Game) -> Vec<Spot> {
    struct R {
        id: String,
        rank: i64,
        pts: i64,
        wins: i64,
        gd: i64,
    }
    let entries = v["standings"]["groups"][0]["standings"]["entries"].as_array();
    let rows: Vec<R> = entries
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let stat = |name: &str| e["stats"].as_array()?.iter().find(|x| x["name"] == name)?["value"].as_f64().map(|x| x as i64);
            Some(R { id: s(&e["id"]), rank: stat("rank")?, pts: stat("points")?, wins: stat("wins").unwrap_or(0), gd: stat("pointDifferential").unwrap_or(0) })
        })
        .collect();
    let goals = |t: &Team| t.score.as_deref().and_then(|x| x.parse::<i64>().ok());
    // Places by points, wins and goal difference, the way the table's before is ranked too.
    let ranked = |mut list: Vec<(String, (i64, i64, i64))>| -> Vec<(String, i64)> {
        list.sort_by_key(|x| std::cmp::Reverse(x.1));
        list.into_iter().enumerate().map(|(i, (id, _))| (id, i as i64 + 1)).collect()
    };
    let after = ranked(rows.iter().map(|r| (r.id.clone(), (r.pts, r.wins, r.gd))).collect());
    let before: Option<Vec<(String, i64)>> = match (game.phase, goals(&game.home), goals(&game.away)) {
        (Phase::Post, Some(h), Some(a)) => {
            let undo = |r: &R| -> (i64, i64, i64) {
                let (us, them) = if r.id == game.home.id {
                    (h, a)
                } else if r.id == game.away.id {
                    (a, h)
                } else {
                    return (r.pts, r.wins, r.gd);
                };
                let (pts, wins) = match us.cmp(&them) {
                    std::cmp::Ordering::Greater => (3, 1),
                    std::cmp::Ordering::Equal => (1, 0),
                    std::cmp::Ordering::Less => (0, 0),
                };
                (r.pts - pts, r.wins - wins, r.gd - (us - them))
            };
            Some(ranked(rows.iter().map(|r| (r.id.clone(), undo(r))).collect()))
        }
        _ => None,
    };
    let place = |list: &[(String, i64)], id: &str| list.iter().find(|(x, _)| x == id).map(|(_, n)| *n);
    [&game.home, &game.away]
        .into_iter()
        .filter_map(|t| {
            let r = rows.iter().find(|r| r.id == t.id)?;
            let moved = match (before.as_deref().and_then(|b| place(b, &t.id)), place(&after, &t.id)) {
                (Some(was), Some(now)) => was - now,
                _ => 0,
            };
            Some(Spot { team: t.id.clone(), rank: r.rank, points: r.pts, moved })
        })
        .collect()
}

/// How far up the pitch a position plays, keeper 0 to striker 4.
fn depth(pos: &str) -> f64 {
    let p = pos.split('-').next().unwrap_or(pos);
    match p {
        "G" | "GK" => 0.0,
        "CD" | "CB" | "D" | "LB" | "RB" | "SW" => 1.0,
        "LWB" | "RWB" | "WB" => 1.5,
        "DM" | "CDM" => 2.0,
        "AM" | "CAM" => 3.0,
        "LW" | "RW" | "F" | "CF" | "ST" | "FW" | "SS" => 4.0,
        _ => 2.5,
    }
}

/// Left to right across the pitch, 0..1.
fn lateral(pos: &str) -> f64 {
    if pos.ends_with("-L") {
        0.35
    } else if pos.ends_with("-R") {
        0.65
    } else if pos.starts_with('L') {
        0.1
    } else if pos.starts_with('R') {
        0.9
    } else {
        0.5
    }
}

/// Starters (jersey, name, position) on the pitch, in the formation's lines.
pub fn place(formation: &str, starters: &[(String, String, String)]) -> Vec<Player> {
    let mut sorted: Vec<&(String, String, String)> = starters.iter().collect();
    sorted.sort_by(|a, b| depth(&a.2).total_cmp(&depth(&b.2)));
    let (keepers, outfield): (Vec<_>, Vec<_>) = sorted.into_iter().partition(|p| depth(&p.2) == 0.0);
    let mut counts: Vec<usize> = formation.split('-').filter_map(|n| n.parse().ok()).collect();
    if counts.iter().sum::<usize>() != outfield.len() {
        // No formation, or it doesn't fit: one line per kind of position.
        counts.clear();
        let mut last = f64::NAN;
        for p in &outfield {
            if depth(&p.2) != last {
                counts.push(0);
                last = depth(&p.2);
            }
            *counts.last_mut().unwrap() += 1;
        }
    }
    let mut out: Vec<Player> = keepers
        .iter()
        .map(|p| Player { n: p.0.clone(), name: p.1.clone(), x: 0.04, y: 0.5 })
        .collect();
    let lines = counts.len().max(1);
    let mut rest = outfield.into_iter();
    for (i, n) in counts.into_iter().enumerate() {
        let mut line: Vec<_> = rest.by_ref().take(n).collect();
        line.sort_by(|a, b| lateral(&a.2).total_cmp(&lateral(&b.2)));
        let x = 0.15 + 0.31 * i as f64 / (lines - 1).max(1) as f64;
        for (j, p) in line.iter().enumerate() {
            let y = if line.len() == 1 { 0.5 } else { 0.12 + 0.76 * j as f64 / (line.len() - 1) as f64 };
            out.push(Player { n: p.0.clone(), name: p.1.clone(), x, y });
        }
    }
    out
}

// ---------- Standings ----------

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub rank: i64,
    pub team: Team,
    pub played: String,
    pub wins: String,
    pub draws: String,
    pub losses: String,
    pub goal_diff: String,
    pub points: String,
    /// The zone's color: qualification, relegation.
    pub zone: Option<String>,
    /// The last five results, oldest first ("WWDLW"), where known.
    pub form: String,
    /// Playing now: places the live score moves the team (up is positive).
    pub live: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Zone {
    pub color: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    /// The group, when there are several.
    pub name: Option<String>,
    pub rows: Vec<Row>,
    pub zones: Vec<Zone>,
}

pub fn parse_standings(v: &Value) -> Vec<Table> {
    let groups: Vec<&Value> = match v["children"].as_array() {
        Some(c) if !c.is_empty() => c.iter().collect(),
        _ => vec![v],
    };
    let several = groups.len() > 1;
    groups
        .into_iter()
        .filter_map(|g| {
            let entries = g["standings"]["entries"].as_array()?;
            let mut zones: Vec<Zone> = Vec::new();
            let mut rows: Vec<Row> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let stat = |name: &str| {
                        e["stats"]
                            .as_array()
                            .and_then(|st| st.iter().find(|x| x["name"] == name))
                            .map(|x| s(&x["displayValue"]))
                            .unwrap_or_default()
                    };
                    let zone = e["note"]["color"].as_str().map(str::to_string);
                    if let (Some(c), Some(label)) = (&zone, e["note"]["description"].as_str()) {
                        if !zones.iter().any(|z| z.color == *c) {
                            zones.push(Zone { color: c.clone(), label: label.to_string() });
                        }
                    }
                    Row {
                        rank: stat("rank").parse().unwrap_or(i as i64 + 1),
                        team: Team { score: None, ..team(e) },
                        played: stat("gamesPlayed"),
                        wins: stat("wins"),
                        draws: stat("ties"),
                        losses: stat("losses"),
                        goal_diff: stat("pointDifferential"),
                        points: stat("points"),
                        zone,
                        form: String::new(),
                        live: None,
                    }
                })
                .collect();
            rows.sort_by_key(|r| r.rank);
            (!rows.is_empty()).then(|| Table {
                name: several.then(|| s(&g["name"])),
                rows,
                zones,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn nba() -> &'static League {
        super::super::league("nba").unwrap()
    }

    #[test]
    fn live_nba_event() {
        let e = json!({
            "id": "401909848", "date": "2026-10-23T20:00Z", "season": { "slug": "regular-season" },
            "status": { "displayClock": "6:56", "period": 3, "type": { "name": "STATUS_IN_PROGRESS", "state": "in", "shortDetail": "6:56 - 3rd" } },
            "competitions": [{
                "venue": { "fullName": "Fiserv Forum" },
                "competitors": [
                    { "homeAway": "home", "score": "80", "team": { "id": "15", "abbreviation": "MIL", "shortDisplayName": "Bucks", "color": "00471b", "logo": "https://x/mil.png" } },
                    { "homeAway": "away", "score": "78", "team": { "id": "4", "abbreviation": "CHI", "shortDisplayName": "Bulls", "color": "ce1141" } }
                ]
            }]
        });
        let g = parse_event(nba(), &e).unwrap();
        assert_eq!(g.phase, Phase::Live);
        assert_eq!(g.status, "Q3 · 6:56");
        assert!((g.progress - (1440.0 + 304.0) / 2880.0).abs() < 1e-3);
        assert_eq!((g.home.name.as_str(), g.home.score.as_deref()), ("Bucks", Some("80")));
        assert_eq!(g.home.color, "#00471b");
        assert_eq!(g.stage.as_deref(), Some("Regular season"));
    }

    #[test]
    fn pre_game_hides_the_zero_score() {
        let e = json!({
            "id": "1", "date": "2026-11-01T21:30Z",
            "status": { "type": { "name": "STATUS_SCHEDULED", "state": "pre" } },
            "competitions": [{
                "series": { "title": "Semifinals" }, "notes": [{ "headline": "1st Leg" }],
                "competitors": [
                    { "homeAway": "home", "score": "0", "team": { "id": "6273", "abbreviation": "GRE", "shortDisplayName": "Grêmio" } },
                    { "homeAway": "away", "score": "0", "team": { "id": "7632", "abbreviation": "CAM", "shortDisplayName": "Atlético-MG" } }
                ]
            }]
        });
        let g = parse_event(super::super::league("bra.copa_do_brazil").unwrap(), &e).unwrap();
        assert_eq!(g.phase, Phase::Pre);
        assert_eq!(g.home.score, None);
        assert_eq!(g.stage.as_deref(), Some("Semifinals · 1st Leg"));
    }

    #[test]
    fn lineup_lines() {
        let p = |n: &str, pos: &str| (n.to_string(), format!("#{n}"), pos.to_string());
        let starters = vec![
            p("1", "G"), p("2", "RB"), p("4", "CD-L"), p("6", "CD-R"), p("3", "LB"), p("5", "DM"), p("8", "DM"),
            p("7", "RM"), p("10", "AM"), p("11", "LM"), p("9", "F"),
        ];
        let placed = place("4-2-3-1", &starters);
        assert_eq!(placed.len(), 11);
        assert_eq!((placed[0].n.as_str(), placed[0].x), ("1", 0.04));
        let back: Vec<&str> = placed[1..5].iter().map(|p| p.n.as_str()).collect();
        assert_eq!(back, ["3", "4", "6", "2"]);
        assert!(placed[10].x > placed[5].x);
        assert_eq!(placed[10].n, "9");
        // Without a formation, by position.
        assert_eq!(place("", &starters).len(), 11);
    }

    fn soccer_game(hs: &str, as_: &str) -> Game {
        let mut g = super::super::tests::sample_game("Internacional", "Botafogo", hs, as_);
        g.home.id = "1936".into();
        g.away.id = "6086".into();
        g
    }

    #[test]
    fn momentum_by_spell() {
        let play = |secs: f64, kind: &str, team: &str| json!({ "time": { "value": secs }, "play": { "type": { "text": kind }, "team": { "displayName": team } } });
        let v = json!({ "commentary": [
            play(100.0, "Shot On Target", "Internacional"), play(120.0, "Corner Awarded", "Internacional"),
            play(400.0, "Shot Off Target", "Botafogo"), play(410.0, "Foul", "Botafogo"), play(700.0, "Kickoff", "")
        ] });
        let m = momentum(&v, &soccer_game("0", "0"));
        // Up to the spell of the last moment; a quiet match stays well inside the chart.
        assert_eq!(m.len(), 4);
        assert_eq!(m[0], 0.67);
        assert_eq!(m[2], -0.33);
        assert_eq!(m[1], 0.0);
    }

    #[test]
    fn table_after_the_result() {
        let row = |id: &str, rank: i64, pts: i64, wins: i64, gd: i64| json!({ "id": id, "stats": [
            { "name": "rank", "value": rank }, { "name": "points", "value": pts },
            { "name": "wins", "value": wins }, { "name": "pointDifferential", "value": gd } ] });
        let v = json!({ "standings": { "groups": [{ "standings": { "entries": [
            row("1", 10, 38, 10, 2), row("6086", 11, 37, 10, 0), row("1936", 12, 37, 10, 1), row("2", 13, 36, 9, 0)
        ] } }] } });
        let mut g = soccer_game("3", "0");
        // Before the 3–0: Internacional on 34 points, last of the four.
        let t = spots(&v, &g);
        assert_eq!((t[0].rank, t[0].points, t[0].moved), (12, 37, 2));
        assert_eq!((t[1].rank, t[1].moved), (11, -1));
        g.phase = Phase::Live;
        assert_eq!(spots(&v, &g)[0].moved, 0);
    }

    #[test]
    fn standings_rows() {
        let v = json!({ "children": [{ "name": "2026", "standings": { "entries": [
            { "team": { "id": "6273", "abbreviation": "GRE", "displayName": "Grêmio" },
              "stats": [{ "name": "rank", "displayValue": "15" }, { "name": "points", "displayValue": "28" }, { "name": "gamesPlayed", "displayValue": "31" }] },
            { "team": { "id": "819", "abbreviation": "FLA", "displayName": "Flamengo" }, "note": { "color": "#81D6AC", "description": "Libertadores" },
              "stats": [{ "name": "rank", "displayValue": "1" }, { "name": "points", "displayValue": "67" }] }
        ] } }] });
        let t = parse_standings(&v);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name, None);
        assert_eq!(t[0].rows[0].team.abbr, "FLA");
        assert_eq!(t[0].rows[1].points, "28");
        assert_eq!(t[0].zones.len(), 1);
    }
}
