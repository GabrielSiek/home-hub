//! DeepInfra's OpenAI-compatible chat completions, with tool calling.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use tvl_kit::{Error, Result};

pub const API: &str = "https://api.deepinfra.com/v1";
/// Fast and good at tool calling; any DeepInfra chat model that supports tools works.
pub const DEFAULT_MODEL: &str = "deepseek-ai/DeepSeek-V4-Flash-0731";
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON text, as the model wrote it.
    #[serde(default)]
    pub arguments: String,
}

fn function_type() -> String {
    "function".into()
}

impl Message {
    pub fn tool_calls(&self) -> &[ToolCall] {
        self.tool_calls.as_deref().unwrap_or_default()
    }

    /// The reply text, without a reasoning model's `<think>` block.
    pub fn text(&self) -> String {
        strip_think(self.content.as_deref().unwrap_or_default())
    }

    /// The message as it goes back into the conversation.
    pub fn to_history(&self) -> Value {
        let mut m = json!({ "role": "assistant", "content": self.text() });
        if !self.tool_calls().is_empty() {
            m["tool_calls"] = json!(self.tool_calls());
        }
        m
    }
}

/// One chat turn. `tools` empty: the model has to answer in text.
pub async fn chat(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    messages: &[Value],
    tools: &[Value],
) -> Result<Message> {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "temperature": 0.4,
        "max_tokens": 800,
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }
    let resp = http
        .post(format!("{API}/openai/chat/completions"))
        .bearer_auth(key)
        .timeout(TIMEOUT)
        .json(&body)
        .send()
        .await?;
    let resp: ChatResponse = checked(resp).await?.json().await?;
    resp.choices
        .into_iter()
        .next()
        .map(|c| c.message)
        .ok_or_else(|| Error::msg("The assistant sent an empty answer"))
}

/// The response when it succeeded, else an error carrying DeepInfra's own message.
pub async fn checked(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            let d = v.pointer("/error/message").or_else(|| v.get("detail"))?;
            Some(d.as_str().map(str::to_string).unwrap_or_else(|| d.to_string()))
        })
        .unwrap_or(body);
    let hint = if status == reqwest::StatusCode::UNAUTHORIZED {
        " (check the DeepInfra API key)"
    } else {
        ""
    };
    let detail: String = detail.chars().take(200).collect();
    Err(Error::msg(format!("DeepInfra {status}{hint}: {detail}")))
}

fn strip_think(s: &str) -> String {
    let s = match (s.find("<think>"), s.find("</think>")) {
        (Some(a), Some(b)) if a < b => format!("{}{}", &s[..a], &s[b + "</think>".len()..]),
        // Only the closing tag: the opening one went into the template.
        (None, Some(b)) => s[b + "</think>".len()..].to_string(),
        _ => s.to_string(),
    };
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_calls_and_null_fields() {
        let r: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"set_volume","arguments":"{\"level\":30}"}}
            ]}}]}"#,
        )
        .unwrap();
        let m = &r.choices[0].message;
        assert_eq!(m.text(), "");
        assert_eq!(m.tool_calls()[0].function.name, "set_volume");
        let h = m.to_history();
        assert_eq!(h["tool_calls"][0]["id"], "call_1");

        let r: ChatResponse =
            serde_json::from_str(r#"{"choices":[{"message":{"content":"Oi!","tool_calls":null}}]}"#).unwrap();
        assert!(r.choices[0].message.tool_calls().is_empty());
        assert!(r.choices[0].message.to_history().get("tool_calls").is_none());
    }

    #[test]
    fn think_blocks_are_dropped() {
        assert_eq!(strip_think("<think>hmm</think>\n\nDone."), "Done.");
        assert_eq!(strip_think("hmm</think>Done."), "Done.");
        assert_eq!(strip_think(" Plain "), "Plain");
    }
}
