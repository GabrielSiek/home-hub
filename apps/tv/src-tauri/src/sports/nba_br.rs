//! Lakers Brasil's "Onde assistir NBA hoje": the day's NBA games in Brasília time with the
//! Brazilian channel of each (ESPN, Disney+, Prime Video or League Pass), updated every morning.

use serde::{Deserialize, Serialize};

use super::dates::{parse_hm, Day};

pub const URL: &str = "https://lakersbrasil.com/onde-assistir-nba/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrGame {
    /// Brasília time.
    pub hour: i64,
    pub minute: i64,
    /// Team codes from the badges ("nyk", "bos") and names ("Knicks").
    pub away: String,
    pub home: String,
    pub away_name: String,
    pub home_name: String,
    /// As written: "Prime", "League Pass", "ESPN · Disney+".
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slate {
    pub day: Day,
    pub games: Vec<BrGame>,
}

enum Token {
    Text(String),
    /// A team badge: the code in its file name, `escudo-nyk-nba.png`.
    Badge(String),
}

fn decode(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&#8211;", "–")
        .replace("&#43;", "+")
        .trim()
        .to_string()
}

/// Text nodes and team badges, in order.
fn tokens(html: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let text = decode(&rest[..lt]);
        if !text.is_empty() {
            out.push(Token::Text(text));
        }
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        let tag = &rest[lt..lt + gt];
        if tag.starts_with("<img") {
            if let Some(at) = tag.find("escudo-") {
                let code = &tag[at + "escudo-".len()..];
                if let Some(end) = code.find("-nba") {
                    out.push(Token::Badge(code[..end].to_lowercase()));
                }
            }
        }
        rest = &rest[lt + gt + 1..];
    }
    out
}

/// The year of "Atualizado em 23 de outubro de 2026" (the date may sit in a tag of its own).
fn updated_year(html: &str) -> Option<i64> {
    html.match_indices("Atualizado em").find_map(|(at, _)| {
        let text: String = tokens(html.get(at..at + 200).unwrap_or(&html[at..]))
            .into_iter()
            .filter_map(|t| match t {
                Token::Text(s) => Some(s),
                Token::Badge(_) => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        text.split(|c: char| !c.is_ascii_digit())
            .filter_map(|w| w.parse::<i64>().ok())
            .find(|y| (2000..2100).contains(y))
    })
}

pub fn parse(html: &str, fallback_year: i64) -> Option<Slate> {
    let start = html.find("Jogos de hoje")?;
    let year = updated_year(html).unwrap_or(fallback_year);
    let mut day = None;
    let mut games = Vec::new();
    // The row being read: time, then badge + name twice, then the channel.
    let mut row: Option<(i64, i64)> = None;
    let mut teams: Vec<(String, String)> = Vec::new();
    let mut want_name = false;
    for t in tokens(&html[start..]) {
        match t {
            Token::Badge(code) => {
                teams.push((code, String::new()));
                want_name = true;
            }
            Token::Text(s) => {
                if s.starts_with("Horários de Brasília") || s.starts_with("Guia do Lakers") {
                    break;
                }
                if day.is_none() {
                    // "Sexta-feira, 23 de outubro"
                    if let Some((_, date)) = s.split_once(", ") {
                        day = Day::parse_pt(date, year);
                    }
                    continue;
                }
                if want_name {
                    if let Some(last) = teams.last_mut() {
                        last.1 = s;
                    }
                    want_name = false;
                } else if s.contains('h') && parse_hm(&s).is_some() && s.len() <= 5 {
                    row = parse_hm(&s);
                    teams.clear();
                } else if teams.len() == 2 && !s.starts_with("Jogo do") {
                    if let Some((hour, minute)) = row.take() {
                        let [(away, away_name), (home, home_name)] = [teams[0].clone(), teams[1].clone()];
                        games.push(BrGame { hour, minute, away, home, away_name, home_name, channel: s });
                    }
                    teams.clear();
                }
            }
        }
    }
    Some(Slate { day: day?, games })
}

/// Lakers Brasil's team code as ESPN's abbreviation, in lower case.
pub fn espn_code(code: &str) -> &str {
    match code {
        "nyk" => "ny",
        "sas" => "sa",
        "nop" => "no",
        "uta" => "utah",
        "gsw" => "gs",
        "was" => "wsh",
        "bkn" | "brk" => "bkn",
        "pho" => "phx",
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(time: &str, away: &str, home: &str, lakers: bool, channel: &str) -> String {
        let team = |code: &str| {
            format!(
                r#"<div><img src="data:image/gif;base64,R0" alt="Escudo" data-src="https://x/thumbor/52x52/https%3A%2F%2Fmedia.lakersbrasil.com%2Fmain%2F2026%2F09%2Fescudo-{code}-nba.png" class="lazy"><span>{}</span></div>"#,
                code.to_uppercase()
            )
        };
        let tag = if lakers { "<div>Jogo do Lakers</div>" } else { "" };
        format!(
            r#"<div><div>{time}</div><div>{}{}{tag}</div><div><span>{channel}</span></div></div>"#,
            team(away),
            team(home)
        )
    }

    #[test]
    fn daily_list() {
        let html = format!(
            "<p>Atualizado em 23 de outubro de 2026.</p><div>Jogos de hoje</div><div>Sexta-feira, 23 de outubro</div><div>12 jogos na rodada.</div>{}{}{}<div>Horários de Brasília. Jogo sem transmissão.</div><div>20h</div>",
            row("20h", "nyk", "bos", false, "Prime"),
            row("22h30", "hou", "sas", false, "ESPN · Disney+"),
            row("23h", "lac", "lal", true, "League Pass"),
        );
        let slate = parse(&html, 2000).unwrap();
        assert_eq!(slate.day, Day { y: 2026, m: 10, d: 23 });
        assert_eq!(slate.games.len(), 3);
        let g = &slate.games[0];
        assert_eq!((g.hour, g.minute, g.away.as_str(), g.home.as_str()), (20, 0, "nyk", "bos"));
        assert_eq!((g.away_name.as_str(), g.channel.as_str()), ("NYK", "Prime"));
        assert_eq!((slate.games[1].hour, slate.games[1].minute), (22, 30));
        assert_eq!(slate.games[1].channel, "ESPN · Disney+");
        assert_eq!(slate.games[2].home, "lal");
        assert_eq!(slate.games[2].channel, "League Pass");
        assert_eq!(espn_code("nyk"), "ny");
    }

    #[test]
    fn not_the_page() {
        assert!(parse("<html>nothing here</html>", 2026).is_none());
    }
}
