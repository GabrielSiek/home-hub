//! Where each Brazilian channel or service is watched, and how the launcher opens it: one of
//! apps.toml's web apps, at a deep link when there is one, or a page in the launcher's browser.

use serde::{Deserialize, Serialize};

/// Globoplay's sports page: the live channels, when ge names no signal for the match.
const GLOBOPLAY_SPORTS: &str = "https://globoplay.globo.com/categorias/esportes/";
/// The day's games, each with its Watch button for League Pass subscribers.
const LEAGUE_PASS: &str = "https://www.nba.com/games";
const PARAMOUNT: &str = "https://www.paramountplus.com/br/";
/// YouTube channels that stream games for free, opened in the YouTube TV app.
const CAZETV: &str = "UCZiYbVptd3PVPf4f6eR6UaQ";
const GE_TV: &str = "UCgCKagVhzGnZcuP9bSMgMCg";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    /// apps.toml app it opens in.
    pub app: Option<String>,
    /// Where: the app's start page when empty. Neither set: shown, but not watchable from here.
    pub url: Option<String>,
    /// A live channel the launcher knows the programme of (tv.rs): it plays with the live
    /// overlay, the channel rail and the score.
    #[serde(default)]
    pub tv: Option<String>,
}

impl Channel {
    fn new(name: &str, app: Option<&str>, url: Option<String>) -> Self {
        Self {
            name: name.to_string(),
            app: app.map(str::to_string),
            url,
            tv: None,
        }
    }

    /// A live channel from tv.rs, by its key.
    pub fn live(name: &str, app: &str, url: Option<String>, key: &str) -> Self {
        Self { tv: Some(key.to_string()), ..Self::new(name, Some(app), url) }
    }

    pub fn watchable(&self) -> bool {
        self.app.is_some() || self.url.is_some()
    }
}

/// Lower case, without accents or spaces: how channel names are compared.
fn key(name: &str) -> String {
    super::fold(name).replace(' ', "")
}

fn youtube_channel(id: &str) -> Option<String> {
    Some(format!("https://www.youtube.com/tv#/browse?c={id}"))
}

/// A channel from ge's "onde assistir". `videos` are the Globoplay live signals ge lists for the
/// match (`gameVideos`: "premiere", "sportv", "tvg"), `link` ge's own link with its call to
/// action. None for what isn't a broadcast (Cartola, tickets).
pub fn from_ge(name: &str, cta: &str, link: Option<&str>, videos: &[(String, u64)]) -> Option<Channel> {
    let signal = |kind: &str| {
        let id = videos.iter().find(|(k, _)| k == kind).map(|(_, id)| *id);
        Some(match id {
            Some(id) => format!("https://globoplay.globo.com/v/{id}/"),
            None => GLOBOPLAY_SPORTS.to_string(),
        })
    };
    let k = key(name);
    let ch = match k.as_str() {
        "cartola" => return None,
        _ if k.starts_with("ingresso") || k.starts_with("socio") => return None,
        "premiere" => Channel::new("Premiere", Some("globoplay"), signal("premiere")),
        "globo" | "tvglobo" => Channel::new("Globo", Some("globoplay"), signal("tvg")),
        "globoplay" => Channel::new("Globoplay", Some("globoplay"), Some(GLOBOPLAY_SPORTS.into())),
        _ if k.starts_with("sportv") => Channel::new(name, Some("globoplay"), signal("sportv")),
        _ => known(name).unwrap_or_else(|| {
            // Something else ge says is free to watch: its own link.
            let url = (cta == "Assista").then(|| link.map(str::to_string)).flatten();
            Channel::new(name, None, url)
        }),
    };
    Some(ch)
}

/// Channels named the same way by ge and Lakers Brasil.
fn known(name: &str) -> Option<Channel> {
    let k = key(name);
    let ch = match k.as_str() {
        "prime" | "primevideo" | "amazonprimevideo" => Channel::new("Prime Video", Some("prime"), None),
        "disney+" | "disneyplus" => Channel::new("Disney+", Some("disney"), None),
        _ if k.starts_with("espn") => Channel::new(name, Some("disney"), None),
        "hbomax" | "max" => Channel::new("HBO Max", Some("hbomax"), None),
        _ if k.starts_with("tnt") => Channel::new(name, Some("hbomax"), None),
        "paramount+" | "paramountplus" | "paramount" =>Channel::new("Paramount+", None, Some(PARAMOUNT.into())),
        "cazetv" => Channel::new("CazéTV", Some("youtube"), youtube_channel(CAZETV)),
        "getv" => Channel::new("ge tv", Some("youtube"), youtube_channel(GE_TV)),
        "youtube" => Channel::new("YouTube", Some("youtube"), None),
        "leaguepass" | "nbaleaguepass" => Channel::new("League Pass", None, Some(LEAGUE_PASS.into())),
        _ => return None,
    };
    Some(ch)
}

/// A game's channel from Lakers Brasil's daily list ("Prime", "League Pass", "ESPN · Disney+").
pub fn nba(label: &str) -> Vec<Channel> {
    label
        .split(['·', '/', ',', '+'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            // "Disney" lost its "+" to the split.
            let s = if s.eq_ignore_ascii_case("disney") { "Disney+" } else { s };
            known(s).unwrap_or_else(|| Channel::new(s, None, None))
        })
        .collect()
}

/// No Brazilian channel listed: every NBA game is on League Pass.
pub fn league_pass() -> Channel {
    known("League Pass").expect("League Pass is a known channel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ge_channels_open_the_match_signal() {
        let videos = vec![("premiere".to_string(), 6942200), ("tvg".to_string(), 7580169)];
        let p = from_ge("Premiere", "Assine", Some("https://premiere.globo.com/"), &videos).unwrap();
        assert_eq!(p.app.as_deref(), Some("globoplay"));
        assert_eq!(p.url.as_deref(), Some("https://globoplay.globo.com/v/6942200/"));
        let g = from_ge("Globo", "Programação", None, &videos).unwrap();
        assert_eq!(g.url.as_deref(), Some("https://globoplay.globo.com/v/7580169/"));
        let s = from_ge("sportv", "Assine", None, &videos).unwrap();
        assert_eq!(s.url.as_deref(), Some(GLOBOPLAY_SPORTS));
        let prime = from_ge("Prime Vídeo", "Assine", None, &[]).unwrap();
        assert_eq!((prime.name.as_str(), prime.app.as_deref()), ("Prime Video", Some("prime")));
        assert!(from_ge("Cartola", "Assine", None, &[]).is_none());
        assert!(from_ge("Ingressos Flamengo", "Compre", None, &[]).is_none());
        assert_eq!(from_ge("Ge TV", "Assista", None, &[]).unwrap().app.as_deref(), Some("youtube"));
        let band = from_ge("Band", "Programação", Some("https://band.com.br"), &[]).unwrap();
        assert!(!band.watchable());
    }

    #[test]
    fn lakers_brasil_labels() {
        let prime = nba("Prime");
        assert_eq!(prime.len(), 1);
        assert_eq!(prime[0].name, "Prime Video");
        let both = nba("ESPN · Disney+");
        assert_eq!(both.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["ESPN", "Disney+"]);
        assert_eq!(nba("League Pass")[0].url.as_deref(), Some(LEAGUE_PASS));
    }
}
