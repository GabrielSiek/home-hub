//! What someone said, as text: Whisper on DeepInfra. The audio comes from a phone or tablet's own
//! microphone (over HTTPS) or a TV PC's; either way it's sent here, where the key is. From the old
//! launcher's `assistant/voice.rs` (its recording stays on the TV PC).

use std::time::Duration;

use serde::Deserialize;
use tvl_kit::Result;

use super::llm::{checked, API};
use crate::ctx::Ctx;

const STT_MODEL: &str = "openai/whisper-large-v3-turbo";
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
struct Transcript {
    #[serde(default)]
    text: String,
}

/// The words in `wav` (16-bit WAV), or None when nothing was said.
pub async fn transcribe_wav(app: &Ctx, wav: Vec<u8>) -> Result<Option<String>> {
    let key = super::api_key(app)?;
    let language = app.settings.get().language;
    let text = transcribe(&tvl_kit::http::api(), &key, &language, wav).await?;
    Ok(Some(text).filter(|t| !t.is_empty()))
}

/// Words Whisper reads as what came before the recording: the launcher's own names, spelt right.
fn hint(lang: &str) -> Option<&'static str> {
    match lang {
        "pt" => Some("Oi! Abre o Globoplay e põe o jogo do Grêmio na CazéTV. Toca no Spotify. Liga a luz da sala."),
        _ => None,
    }
}

/// `language`: the household's ("pt-BR"). Left to guess from a few seconds of speech, Whisper
/// often takes Portuguese for Spanish or English and writes down something else.
async fn transcribe(http: &reqwest::Client, key: &str, language: &str, wav: Vec<u8>) -> Result<String> {
    let part = reqwest::multipart::Part::bytes(wav).file_name("speech.wav").mime_str("audio/wav")?;
    let lang = language.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
    let mut form = reqwest::multipart::Form::new().part("audio", part);
    if let Some(h) = hint(&lang) {
        form = form.text("initial_prompt", h);
    }
    if !lang.is_empty() {
        form = form.text("language", lang);
    }
    let resp = http.post(format!("{API}/inference/{STT_MODEL}")).bearer_auth(key).timeout(TIMEOUT).multipart(form).send().await?;
    let t: Transcript = checked(resp).await?.json().await?;
    Ok(t.text.trim().to_string())
}
