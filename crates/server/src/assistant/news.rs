//! Headlines from Google News' RSS feeds (no key needed), in the launcher's language and region.

use serde::Serialize;

use tvl_kit::Result;

const MAX_ITEMS: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Headline {
    pub title: String,
    pub source: Option<String>,
    pub published: Option<String>,
}

/// Top stories, or the latest about `topic`.
pub async fn headlines(
    http: &reqwest::Client,
    topic: Option<&str>,
    language: &str,
    region: &str,
) -> Result<Vec<Headline>> {
    let (hl, gl, ceid) = edition(language, region);
    let mut req = match topic.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => http.get("https://news.google.com/rss/search").query(&[("q", t)]),
        None => http.get("https://news.google.com/rss"),
    };
    req = req.query(&[("hl", hl.as_str()), ("gl", gl.as_str()), ("ceid", ceid.as_str())]);
    let xml = req.send().await?.error_for_status()?.text().await?;
    Ok(parse(&xml))
}

/// Google News' edition parameters. Brazil's Portuguese edition is "pt-419".
fn edition(language: &str, region: &str) -> (String, String, String) {
    let gl = region.trim().to_uppercase();
    let gl = if gl.is_empty() { "US".to_string() } else { gl };
    let lang = language.trim();
    let short = lang.split('-').next().unwrap_or("en").to_lowercase();
    let ceid_lang = if short == "pt" && gl == "BR" { "pt-419".to_string() } else { short };
    (lang.to_string(), gl.clone(), format!("{gl}:{ceid_lang}"))
}

fn parse(xml: &str) -> Vec<Headline> {
    xml.split("<item>")
        .skip(1)
        .filter_map(|item| {
            let raw = tag(item, "title")?;
            let source = tag(item, "source");
            // Google appends " - Source" to each title; the source has its own tag.
            let title = match &source {
                Some(s) => raw.strip_suffix(&format!(" - {s}")).unwrap_or(&raw).to_string(),
                None => raw.clone(),
            };
            Some(Headline {
                title,
                source,
                published: tag(item, "pubDate"),
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

/// The text of the first `<name …>…</name>` element, unescaped.
fn tag(s: &str, name: &str) -> Option<String> {
    let open = s.find(&format!("<{name}"))?;
    let start = open + s[open..].find('>')? + 1;
    let end = start + s[start..].find(&format!("</{name}>"))?;
    let text = s[start..end].trim();
    let text = text
        .strip_prefix("<![CDATA[")
        .and_then(|t| t.strip_suffix("]]>"))
        .unwrap_or(text);
    Some(unescape(text)).filter(|t| !t.is_empty())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editions() {
        assert_eq!(
            edition("pt-BR", "BR"),
            ("pt-BR".into(), "BR".into(), "BR:pt-419".into())
        );
        assert_eq!(edition("en-US", "us"), ("en-US".into(), "US".into(), "US:en".into()));
    }

    #[test]
    fn parses_items() {
        let xml = r#"<rss><channel><title>Top stories</title>
            <item><title>Chuva forte em SP &amp; alertas - G1</title><link>x</link>
              <pubDate>Thu, 22 Oct 2026 12:00:00 GMT</pubDate><source url="https://g1.globo.com">G1</source></item>
            <item><title><![CDATA[Plain title]]></title></item>
            <item><link>no title</link></item>
        </channel></rss>"#;
        let items = parse(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Chuva forte em SP & alertas");
        assert_eq!(items[0].source.as_deref(), Some("G1"));
        assert_eq!(items[0].published.as_deref(), Some("Thu, 22 Oct 2026 12:00:00 GMT"));
        assert_eq!(items[1].title, "Plain title");
        assert!(items[1].source.is_none());
    }

    /// Calls Google News: `cargo test -- --ignored real_news`.
    #[test]
    #[ignore]
    fn real_news() {
        let http = reqwest::Client::new();
        let top = tokio::runtime::Runtime::new().unwrap().block_on(headlines(&http, None, "pt-BR", "BR")).unwrap();
        assert!(!top.is_empty());
        let topic = tokio::runtime::Runtime::new().unwrap().block_on(headlines(&http, Some("futebol"), "pt-BR", "BR")).unwrap();
        println!("{:?} / {:?}", &top[..2], &topic[..1]);
    }
}
