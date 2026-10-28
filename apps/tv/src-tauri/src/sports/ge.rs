//! ge (globo esporte): the pages embed their data as JSON. Grêmio's agenda gives every match
//! of the season with its Brazilian channels, the Globoplay signals that show it and the
//! highlights video; the daily agenda the same for every game from two days ago to two days
//! ahead; the Brasileirão page its table.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::channels::{self, Channel};
use super::dates::{parse_hm, Day};

pub const GREMIO_URL: &str = "https://ge.globo.com/rs/futebol/times/gremio/agenda-de-jogos-do-gremio/";
pub const AGENDA_URL: &str = "https://ge.globo.com/agenda/";
pub const TABLE_URL: &str = "https://ge.globo.com/futebol/brasileirao-serie-a/";
/// Grêmio in ge's data.
pub const GREMIO_ID: u64 = 284;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeTeam {
    pub id: u64,
    pub name: String,
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeMatch {
    pub id: u64,
    pub championship: String,
    /// "Semifinal", "Fase única".
    pub phase: String,
    pub round: Option<i64>,
    pub day: Day,
    /// Unix ms, when the kick-off time is set.
    pub start: Option<i64>,
    pub home: GeTeam,
    pub away: GeTeam,
    pub score: Option<(i64, i64)>,
    pub penalties: Option<(i64, i64)>,
    /// Being played, by ge's account (PAST / NOW / FUTURE).
    pub live: bool,
    pub venue: Option<String>,
    pub channels: Vec<Channel>,
    /// The highlights on Globoplay.
    pub video: Option<String>,
    /// ge's page for the match.
    pub url: Option<String>,
}

/// The JSON value right after `marker` in `html`.
fn json_after(html: &str, marker: &str) -> Option<Value> {
    let at = html.find(marker)? + marker.len();
    serde_json::Deserializer::from_str(&html[at..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

fn team(v: &Value) -> GeTeam {
    GeTeam {
        id: v["id"].as_u64().unwrap_or_default(),
        name: v["popularName"].as_str().or(v["name"].as_str()).unwrap_or_default().to_string(),
        badge: v["badgeSvg"].as_str().or(v["badgePng"].as_str()).map(str::to_string),
    }
}

fn pair(v: &Value) -> Option<(i64, i64)> {
    Some((v["home"].as_i64()?, v["away"].as_i64()?))
}

/// One `SoccerEvent`; None for other sports and for matches without a date.
fn parse_event(e: &Value, championship: Option<&str>) -> Option<GeMatch> {
    let m = &e["match"];
    if m.is_null() {
        return None;
    }
    let day = Day::parse_iso(m["startDate"].as_str()?)?;
    let start = m["startHour"].as_str().and_then(parse_hm).map(|(h, mi)| day.brt_ms(h, mi));
    let tr = &m["transmission"];
    let videos: Vec<(String, u64)> = tr["gameVideos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some((g["mediaType"].as_str()?.to_string(), g["video"]["id"].as_u64()?)))
        .collect();
    let channels = m["liveWatchSources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            channels::from_ge(
                s["name"].as_str()?,
                s["cta"].as_str().unwrap_or_default(),
                s["url"].as_str(),
                &videos,
            )
        })
        .collect();
    let past = m["moment"].as_str() == Some("PAST");
    Some(GeMatch {
        id: m["id"].as_u64()?,
        championship: championship
            .or(m["phase"]["championshipEdition"]["championship"]["name"].as_str())
            .unwrap_or_default()
            .to_string(),
        phase: m["phase"]["name"].as_str().unwrap_or_default().to_string(),
        round: m["round"].as_i64(),
        day,
        start,
        home: team(&m["firstContestant"]),
        away: team(&m["secondContestant"]),
        score: pair(&m["scoreboard"]),
        penalties: pair(&m["scoreboard"]["penalty"]),
        live: m["moment"].as_str() == Some("NOW"),
        venue: m["location"]["popularName"].as_str().map(str::to_string),
        channels,
        video: past
            .then(|| tr["featuredVideo"]["id"].as_u64())
            .flatten()
            .map(|id| format!("https://globoplay.globo.com/v/{id}/")),
        url: tr["url"].as_str().filter(|u| !u.is_empty()).map(str::to_string),
    })
}

fn events(v: &Value) -> impl Iterator<Item = &Value> {
    ["past", "now", "future"]
        .into_iter()
        .flat_map(move |k| v[k].as_array().into_iter().flatten())
}

/// A team's agenda page: the season's matches, oldest first.
pub fn parse_team(html: &str) -> Option<Vec<GeMatch>> {
    let data = json_after(html, "scheduleTeam: ")?;
    let agenda = &data["teamAgenda"];
    agenda.is_object().then(|| events(agenda).filter_map(|e| parse_event(e, None)).collect())
}

/// The daily agenda: every game ge covers, two days back to two days ahead.
pub fn parse_agenda(html: &str) -> Option<Vec<GeMatch>> {
    let start = html.find("window.dataSportsSchedule")?;
    let data = json_after(&html[start..], "sport: ")?;
    let days = data.as_object()?;
    let mut out = Vec::new();
    for day in days.values() {
        for c in day["championshipsAgenda"].as_array().into_iter().flatten() {
            let name = c["championship"]["name"].as_str();
            out.extend(events(c).filter_map(|e| parse_event(e, name)));
        }
    }
    Some(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeRow {
    pub rank: i64,
    pub name: String,
    pub abbr: String,
    pub badge: Option<String>,
    pub played: i64,
    pub wins: i64,
    pub draws: i64,
    pub losses: i64,
    pub goal_diff: i64,
    pub points: i64,
    /// The last five results, oldest first: "WWDLW".
    pub form: String,
}

/// The Brasileirão table on ge's championship page.
pub fn parse_table(html: &str) -> Option<Vec<GeRow>> {
    let data = json_after(html, "const classificacao = ")?;
    let rows: Vec<GeRow> = data["classificacao"]
        .as_array()?
        .iter()
        .filter_map(|r| {
            Some(GeRow {
                rank: r["ordem"].as_i64()?,
                name: r["nome_popular"].as_str()?.to_string(),
                abbr: r["sigla"].as_str().unwrap_or_default().to_string(),
                badge: r["escudo"].as_str().map(str::to_string),
                played: r["jogos"].as_i64().unwrap_or_default(),
                wins: r["vitorias"].as_i64().unwrap_or_default(),
                draws: r["empates"].as_i64().unwrap_or_default(),
                losses: r["derrotas"].as_i64().unwrap_or_default(),
                goal_diff: r["saldo_gols"].as_i64().unwrap_or_default(),
                points: r["pontos"].as_i64().unwrap_or_default(),
                // v(itória), e(mpate), d(errota).
                form: r["ultimos_jogos"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| match x.as_str()? {
                        "v" => Some('W'),
                        "e" => Some('D'),
                        "d" => Some('L'),
                        _ => None,
                    })
                    .collect(),
            })
        })
        .collect();
    (!rows.is_empty()).then_some(rows)
}

/// ge's phase names, in English.
pub fn phase_name(phase: &str) -> String {
    let lower = super::fold(phase);
    let known = [
        ("fase unica", ""),
        ("final", "Final"),
        ("semifinal", "Semifinal"),
        ("quartas de final", "Quarterfinal"),
        ("oitavas de final", "Round of 16"),
        ("fase de grupos", "Group stage"),
        ("primeira fase", "First round"),
        ("segunda fase", "Second round"),
        ("terceira fase", "Third round"),
        ("quarta fase", "Fourth round"),
        ("quinta fase", "Fifth round"),
        ("playoffs", "Playoffs"),
    ];
    known
        .iter()
        .find(|(pt, _)| lower == *pt)
        .map(|(_, en)| en.to_string())
        .unwrap_or_else(|| phase.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM_PAGE: &str = r#"<script>
  window.scheduleTeam = {"enabled":true,"teamId":284};
  window.dataSportsSchedule = {
    sport: {},
    scheduleTeam: {"teamAgenda":{"future":[{"__typename":"SoccerEvent","match":{"firstContestant":{"id":294,"popularName":"Coritiba","badgeSvg":"https://s/coritiba.svg"},"id":346472,"liveWatchSources":[{"cta":"Assine","name":"Premiere","url":"https://premiere.globo.com/"},{"cta":"Assine","name":"Cartola","url":"x"}],"location":{"popularName":"Couto Pereira"},"moment":"FUTURE","phase":{"championshipEdition":{"championship":{"name":"Campeonato Brasileiro"}},"name":"Fase única"},"round":32,"scoreboard":{"away":null,"home":null,"penalty":null},"secondContestant":{"id":284,"popularName":"Grêmio"},"startDate":"2026-10-26","startHour":"19:30:00","transmission":{"gameVideos":[{"mediaType":"premiere","video":{"id":6942200}}],"url":"https://ge.globo.com/jogo/coritiba-gremio.ghtml"}}}],"now":[],"past":[{"match":{"firstContestant":{"id":284,"popularName":"Grêmio"},"id":1,"liveWatchSources":[],"moment":"PAST","phase":{"championshipEdition":{"championship":{"name":"Campeonato Brasileiro"}},"name":"Fase única"},"round":31,"scoreboard":{"away":4,"home":2,"penalty":null},"secondContestant":{"id":2,"popularName":"Cruzeiro"},"startDate":"2026-10-18","startHour":"16:00:00","transmission":{"featuredVideo":{"id":13830630},"gameVideos":[]}}}]}}
  };
</script>"#;

    #[test]
    fn team_agenda() {
        let ms = parse_team(TEAM_PAGE).unwrap();
        assert_eq!(ms.len(), 2);
        let past = &ms[0];
        assert_eq!((past.home.name.as_str(), past.score), ("Grêmio", Some((2, 4))));
        assert_eq!(past.video.as_deref(), Some("https://globoplay.globo.com/v/13830630/"));
        let next = &ms[1];
        assert_eq!(next.away.id, GREMIO_ID);
        assert_eq!(next.round, Some(32));
        assert_eq!(next.start, Some(Day { y: 2026, m: 10, d: 26 }.brt_ms(19, 30)));
        assert_eq!(next.score, None);
        assert_eq!(next.channels.len(), 1);
        assert_eq!(next.channels[0].url.as_deref(), Some("https://globoplay.globo.com/v/6942200/"));
        assert_eq!(next.venue.as_deref(), Some("Couto Pereira"));
        assert_eq!(phase_name(&next.phase), "");
    }

    #[test]
    fn daily_agenda() {
        let html = r#"window.dataSportsSchedule = {
    sport: {"2026-10-24":{"championshipsAgenda":[{"championship":{"name":"Premier League"},"future":[{"match":{"firstContestant":{"id":1,"popularName":"Arsenal"},"id":9,"liveWatchSources":[{"cta":"Assine","name":"ESPN"}],"moment":"FUTURE","phase":{"name":"Fase única"},"scoreboard":{"home":null,"away":null},"secondContestant":{"id":2,"popularName":"Chelsea"},"startDate":"2026-10-24","startHour":"11:00:00","transmission":null}}],"now":[],"past":[]}]}},
    multisport: {},
  };"#;
        let ms = parse_agenda(html).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].championship, "Premier League");
        assert_eq!(ms[0].channels[0].app.as_deref(), Some("disney"));
    }

    #[test]
    fn table() {
        let html = r#"const classificacao = {"classificacao":[{"derrotas":4,"empates":7,"jogos":31,"nome_popular":"Flamengo","ordem":1,"pontos":67,"saldo_gols":36,"sigla":"FLA","ultimos_jogos":["v","v","e","d","v"],"vitorias":20}]};"#;
        let rows = parse_table(html).unwrap();
        assert_eq!((rows[0].abbr.as_str(), rows[0].points, rows[0].played), ("FLA", 67, 31));
        assert_eq!(rows[0].form, "WWDLW");
        assert!(parse_table("nothing").is_none());
    }
}
