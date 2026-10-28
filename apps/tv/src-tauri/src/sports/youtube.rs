//! Highlights on YouTube, found without an API key: the search page embeds its results as
//! JSON (`ytInitialData`). ge tv posts every Brasileirão and Copa do Brasil match, NBA Brasil
//! every NBA game, with both teams and the score in the title.

use serde::Serialize;
use serde_json::Value;

use super::{fold, words, Game, Sport};
use crate::error::{Error, Result};

const SEARCH: &str = "https://www.youtube.com/results";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
/// Channels that post the official highlights.
const TRUSTED: &[&str] = &[
    "ge tv", "ge.globo", "nba", "nba brasil", "cazetv", "espn brasil", "tnt sports brasil", "premiere",
    "sportv", "prime video brasil", "disney+ brasil", "conmebol libertadores", "conmebol sudamericana",
    "uol esporte", "gaucha esportes",
];
const KEYWORDS: &[&str] = &["melhores momentos", "highlights", "gols", "resumo"];
const NOT_HIGHLIGHTS: &[&str] = &[
    "reacao", "reacoes", "react", "ao vivo", "live", "pre jogo", "pos jogo", "coletiva", "narracao", "podcast",
];

#[derive(Debug, Clone, Serialize)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub channel: String,
    /// "6:17".
    pub length: Option<String>,
}

struct Result_ {
    id: String,
    title: String,
    channel: String,
    /// Age in hours, from "há 5 dias" / "5 days ago".
    age_h: Option<f64>,
    /// None for a live stream.
    length: Option<String>,
}

fn runs(v: &Value) -> String {
    v["runs"]
        .as_array()
        .map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect())
        .or_else(|| v["simpleText"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn collect(v: &Value, out: &mut Vec<Result_>) {
    match v {
        Value::Object(o) => {
            if let Some(r) = o.get("videoRenderer") {
                if let Some(id) = r["videoId"].as_str() {
                    out.push(Result_ {
                        id: id.to_string(),
                        title: runs(&r["title"]),
                        channel: runs(&r["ownerText"]),
                        age_h: age_hours(&runs(&r["publishedTimeText"])),
                        length: r["lengthText"]["simpleText"].as_str().map(str::to_string),
                    });
                }
                return;
            }
            o.values().for_each(|x| collect(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
        _ => {}
    }
}

/// "há 5 dias", "Transmitido há 2 horas", "3 weeks ago": hours.
fn age_hours(text: &str) -> Option<f64> {
    let t = fold(text);
    let n: f64 = t.split_whitespace().find_map(|w| w.parse().ok())?;
    let units = [
        ("minut", 1.0 / 60.0),
        ("hora", 1.0),
        ("hour", 1.0),
        ("dia", 24.0),
        ("day", 24.0),
        ("semana", 168.0),
        ("week", 168.0),
        ("mes", 720.0),
        ("month", 720.0),
        ("ano", 8760.0),
        ("year", 8760.0),
    ];
    units.iter().find(|(u, _)| t.contains(u)).map(|(_, h)| n * h)
}

/// The words a title must hold to be about `name`: the whole name, or its longest word.
fn mentions(title: &str, name: &str) -> bool {
    let name = words(name);
    if name.is_empty() {
        return false;
    }
    if has(title, &name) {
        return true;
    }
    name.split(' ')
        .filter(|w| w.len() >= 4)
        .max_by_key(|w| w.len())
        .is_some_and(|w| title.split(' ').any(|t| t == w))
}

/// `phrase` appears in `title` as whole words (both already through `words`).
fn has(title: &str, phrase: &str) -> bool {
    format!(" {title} ").contains(&format!(" {phrase} "))
}

fn score(r: &Result_, game: &Game, since_h: f64) -> Option<i32> {
    let title = words(&r.title);
    let (a, b) = (&game.home, &game.away);
    if !mentions(&title, &a.name) || !mentions(&title, &b.name) {
        return None;
    }
    if !KEYWORDS.iter().any(|k| has(&title, k)) || NOT_HIGHLIGHTS.iter().any(|k| has(&title, k)) {
        return None;
    }
    // A full replay is long; "melhores momentos do jogo completo" (NBA Brasil) is not.
    if has(&title, "jogo completo") && !has(&title, "melhores momentos") {
        return None;
    }
    r.length.as_ref()?;
    // Uploaded after the game (a day of slack for the clock), not an old meeting of the two.
    if r.age_h.is_some_and(|h| h > since_h + 24.0) {
        return None;
    }
    let year = super::dates::Day::brt(game.start).y.to_string();
    if title.split(' ').any(|w| w.len() == 4 && w.starts_with(['1', '2']) && w.parse::<u32>().is_ok() && w != year) {
        return None;
    }
    let mut points = 1;
    if TRUSTED.contains(&fold(&r.channel).as_str()) {
        points += 3;
    }
    if let (Some(x), Some(y)) = (&a.score, &b.score) {
        let s = |p: &str, q: &str| [format!("{p} x {q}"), format!("{p}x{q}"), format!("{p} {q}")];
        if s(x, y).iter().chain(s(y, x).iter()).any(|t| has(&title, t)) {
            points += 2;
        }
    }
    Some(points)
}

/// The game's highlights videos that are up yet, best first, at most three.
pub async fn highlights(http: &reqwest::Client, game: &Game, now: i64) -> Result<Vec<Video>> {
    let (first, second) = match game.sport {
        // NBA Brasil writes "HAWKS x MAGIC": away first.
        Sport::Basketball => (&game.away.name, &game.home.name),
        Sport::Soccer => (&game.home.name, &game.away.name),
    };
    let query = format!("{first} x {second} melhores momentos");
    let html = http
        .get(SEARCH)
        .query(&[("search_query", query.as_str())])
        .header("User-Agent", UA)
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let marker = "var ytInitialData = ";
    let at = html.find(marker).ok_or_else(|| Error::msg("YouTube's search page changed"))? + marker.len();
    let data = serde_json::Deserializer::from_str(&html[at..])
        .into_iter::<Value>()
        .next()
        .ok_or_else(|| Error::msg("YouTube's search page changed"))??;
    let mut results = Vec::new();
    collect(&data, &mut results);
    let since_h = (now - game.start).max(0) as f64 / 3_600_000.0;
    let mut found: Vec<(i32, std::cmp::Reverse<usize>, Result_)> = results
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| score(&r, game, since_h).map(|s| (s, std::cmp::Reverse(i), r)))
        .collect();
    // Ties go to YouTube's own ranking.
    found.sort_by_key(|(s, i, _)| std::cmp::Reverse((*s, *i)));
    let mut out: Vec<Video> = Vec::new();
    for (_, _, r) in found {
        if out.len() < 3 && !out.iter().any(|v| v.id == r.id) {
            out.push(Video { id: r.id, title: r.title, channel: r.channel, length: r.length });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::tests::sample_game;

    fn r(title: &str, channel: &str, age: &str) -> Result_ {
        Result_ {
            id: "x".into(),
            title: title.into(),
            channel: channel.into(),
            age_h: age_hours(age),
            length: Some("6:17".into()),
        }
    }

    #[test]
    fn ages() {
        assert_eq!(age_hours("há 5 dias"), Some(120.0));
        assert_eq!(age_hours("Transmitido há 2 horas"), Some(2.0));
        assert_eq!(age_hours("3 weeks ago"), Some(504.0));
        assert_eq!(age_hours("há 7 meses"), Some(5040.0));
    }

    #[test]
    fn picks_the_official_highlights() {
        let game = sample_game("Grêmio", "Cruzeiro", "2", "4");
        let since = 5.0 * 24.0;
        let official = r("GRÊMIO 2 X 4 CRUZEIRO | MELHORES MOMENTOS | 31ª RODADA | BRASILEIRÃO 2026 | ge.globo", "ge tv", "há 5 dias");
        let fan = r("🔵⚫️ Grêmio 2x4 Cruzeiro: VEJA OS MELHORES MOMENTOS!", "A Dupla", "há 4 dias");
        let old = r("GRÊMIO X CRUZEIRO | MELHORES MOMENTOS | BRASILEIRÃO 2009 | ge tv", "ge tv", "há 5 dias");
        let react = r("REAÇÕES GREMISTAS - GRÊMIO 2x4 CRUZEIRO - MELHORES MOMENTOS", "A Voz da Torcida", "há 5 dias");
        let full = r("JOGO COMPLETO: GRÊMIO X CRUZEIRO | BRASILEIRÃO 2026 | ge tv", "ge tv", "há 4 dias");
        let earlier = r("GRÊMIO 1 X 0 CRUZEIRO | MELHORES MOMENTOS", "ge tv", "há 7 meses");
        assert!(score(&official, &game, since) > score(&fan, &game, since));
        assert!(score(&fan, &game, since).is_some());
        for bad in [&old, &react, &full, &earlier] {
            assert_eq!(score(bad, &game, since), None, "{}", bad.title);
        }
    }
}
