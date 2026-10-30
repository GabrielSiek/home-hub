//! mpv's JSON pipe: one JSON object per line each way. Reading what it says is apart from the pipe
//! itself, so it's tested on what mpv really sends.

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, ReadHalf, WriteHalf};

use super::{PlayerState, Track};

/// How long mpv gets to open its pipe.
#[cfg(windows)]
const OPEN_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The properties followed, in the order of their observe ids.
pub const WATCHED: &[&str] = &["time-pos", "duration", "pause", "paused-for-cache", "demuxer-cache-duration", "volume", "eof-reached", "track-list"];

/// Takes in one line from mpv.
pub fn apply(s: &mut PlayerState, v: &Value) {
    if v["event"] != "property-change" {
        return;
    }
    let data = &v["data"];
    let number = data.as_f64();
    match v["name"].as_str().unwrap_or_default() {
        "time-pos" => s.position = number.unwrap_or(s.position),
        "duration" => s.duration = number.unwrap_or(s.duration),
        "pause" => s.paused = data.as_bool().unwrap_or(s.paused),
        "paused-for-cache" => s.buffering = data.as_bool().unwrap_or(false),
        "demuxer-cache-duration" => s.cached = number.unwrap_or(0.0),
        "volume" => s.volume = number.unwrap_or(s.volume),
        "eof-reached" => s.ended = data.as_bool().unwrap_or(false),
        "track-list" => (s.audio, s.subtitles) = tracks(data),
        _ => {}
    }
}

fn language_name(code: &str) -> Option<&'static str> {
    Some(match code.split(['-', '_']).next()?.to_ascii_lowercase().as_str() {
        "pt" | "por" | "pob" | "pb" => "Português",
        "en" | "eng" => "English",
        "es" | "spa" => "Español",
        "fr" | "fra" | "fre" => "Français",
        "de" | "deu" | "ger" => "Deutsch",
        "it" | "ita" => "Italiano",
        "ja" | "jpn" => "日本語",
        "ko" | "kor" => "한국어",
        "zh" | "zho" | "chi" => "中文",
        _ => return None,
    })
}

fn channels(n: u64) -> Option<&'static str> {
    Some(match n {
        1 => "mono",
        2 => "stereo",
        6 => "5.1",
        8 => "7.1",
        _ => return None,
    })
}

/// A track's name: its language, its title when that says something more, and for audio the format.
fn track_name(t: &Value, n: usize) -> String {
    let title = t["title"].as_str().map(str::trim).filter(|x| !x.is_empty());
    let lang = t["lang"].as_str();
    let language = lang.and_then(language_name).map(str::to_string).or_else(|| lang.map(str::to_uppercase));
    let mut parts: Vec<String> = Vec::new();
    match (language, title) {
        (Some(l), Some(x)) if !x.eq_ignore_ascii_case(&l) => parts.extend([l, x.to_string()]),
        (Some(l), _) => parts.push(l),
        (None, Some(x)) => parts.push(x.to_string()),
        (None, None) => parts.push(format!("Track {n}")),
    }
    if t["type"] == "audio" {
        parts.extend(t["codec"].as_str().map(str::to_uppercase));
        parts.extend(t["demux-channel-count"].as_u64().and_then(channels).map(str::to_string));
    }
    parts.join(" · ")
}

/// mpv's track list as the audio and subtitle choices.
fn tracks(list: &Value) -> (Vec<Track>, Vec<Track>) {
    let (mut audio, mut subs) = (Vec::new(), Vec::new());
    for t in list.as_array().into_iter().flatten() {
        let kind = t["type"].as_str().unwrap_or_default();
        let into = match kind {
            "audio" => &mut audio,
            "sub" => &mut subs,
            _ => continue,
        };
        let name = track_name(t, into.len() + 1);
        into.push(Track { id: t["id"].as_i64().unwrap_or(0), name, on: t["selected"].as_bool().unwrap_or(false) });
    }
    (audio, subs)
}

#[cfg(windows)]
type Raw = tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(not(windows))]
type Raw = tokio::io::DuplexStream;

/// The open pipe.
pub struct Pipe(Raw);

pub struct Reader(Lines<BufReader<ReadHalf<Raw>>>);
pub struct Writer(WriteHalf<Raw>);

impl Pipe {
    pub fn split(self) -> (Reader, Writer) {
        let (r, w) = tokio::io::split(self.0);
        (Reader(BufReader::new(r).lines()), Writer(w))
    }
}

impl Reader {
    /// The next line; None when mpv is gone.
    pub async fn next(&mut self) -> Option<String> {
        self.0.next_line().await.ok().flatten()
    }
}

impl Writer {
    pub async fn send(&mut self, line: &str) -> std::io::Result<()> {
        self.0.write_all(line.as_bytes()).await
    }
}

/// Opens mpv's pipe, waiting for it to appear.
#[cfg(windows)]
pub async fn connect(name: &str) -> std::io::Result<Pipe> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let until = std::time::Instant::now() + OPEN_WAIT;
    loop {
        match ClientOptions::new().open(name) {
            Ok(p) => return Ok(Pipe(p)),
            Err(e) if std::time::Instant::now() >= until => return Err(e),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
}

#[cfg(not(windows))]
pub async fn connect(_name: &str) -> std::io::Result<Pipe> {
    Err(std::io::Error::other("The player works on Windows only"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn change(name: &str, data: Value) -> Value {
        json!({ "event": "property-change", "id": 1, "name": name, "data": data })
    }

    #[test]
    fn position_and_state_follow_what_mpv_reports() {
        let mut s = PlayerState::default();
        apply(&mut s, &change("time-pos", json!(12.5)));
        apply(&mut s, &change("duration", json!(7200.0)));
        apply(&mut s, &change("pause", json!(true)));
        apply(&mut s, &change("eof-reached", json!(true)));
        assert_eq!((s.position, s.duration, s.paused, s.ended), (12.5, 7200.0, true, true));
    }

    #[test]
    fn a_property_with_no_value_yet_keeps_the_last_one() {
        let mut s = PlayerState { position: 50.0, ..Default::default() };
        apply(&mut s, &change("time-pos", Value::Null));
        assert_eq!(s.position, 50.0);
        apply(&mut s, &change("eof-reached", Value::Null));
        assert!(!s.ended);
    }

    #[test]
    fn other_lines_change_nothing() {
        let mut s = PlayerState::default();
        apply(&mut s, &json!({ "event": "playback-restart" }));
        apply(&mut s, &json!({ "request_id": 0, "error": "success" }));
        assert_eq!(s, PlayerState::default());
    }

    #[test]
    fn tracks_are_named_for_people() {
        let list = json!([
            { "id": 1, "type": "video", "codec": "hevc", "selected": true },
            { "id": 1, "type": "audio", "lang": "por", "title": "Dublado", "codec": "ac3", "demux-channel-count": 6, "selected": true },
            { "id": 2, "type": "audio", "lang": "eng", "codec": "dts", "demux-channel-count": 2 },
            { "id": 3, "type": "audio", "codec": "aac" },
            { "id": 1, "type": "sub", "lang": "pt-BR", "title": "pt-BR", "selected": true },
            { "id": 2, "type": "sub", "lang": "nl" },
            { "id": 3, "type": "sub", "title": "Forced" },
        ]);
        let (audio, subs) = tracks(&list);
        let names = |l: &[Track]| l.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&audio), ["Português · Dublado · AC3 · 5.1", "English · DTS · stereo", "Track 3 · AAC"]);
        assert_eq!(names(&subs), ["Português · pt-BR", "NL", "Forced"]);
        assert!(audio[0].on && !audio[1].on);
        assert_eq!(subs[2].id, 3);
    }
}
