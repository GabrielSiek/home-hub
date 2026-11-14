//! The assistant's model, with tool calling: Claude (Anthropic's Messages API) or a DeepInfra
//! model (OpenAI-compatible chat completions).
//!
//! The conversation is kept in the OpenAI shape for both. Claude's turns also carry their
//! content blocks exactly as Claude sent them, under "claude", so its thinking goes back to it
//! unchanged; DeepInfra never sees that field.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use tvl_kit::{Error, Result};

/// DeepInfra, also used for speech recognition (voice.rs).
pub const API: &str = "https://api.deepinfra.com/v1";
/// Fast and good at tool calling; any DeepInfra chat model that supports tools works.
pub const DEFAULT_MODEL: &str = "deepseek-ai/DeepSeek-V4-Flash-0731";
const CLAUDE_API: &str = "https://api.anthropic.com/v1/messages";
pub const CLAUDE_MODEL: &str = "claude-sonnet-5-5";
/// Thinking blocks from an earlier question are dropped rather than refused once the system
/// prompt (clock, device states) has changed; a declined request is retried on another model.
const CLAUDE_BETAS: &str = "thinking-binding-controls-2026-08-01,server-side-fallback-2026-07-01";
/// Thinking counts toward it as well as the reply.
const CLAUDE_MAX_TOKENS: u32 = 4096;
const TIMEOUT: Duration = Duration::from_secs(60);

/// Which model answers, with its key.
pub enum Backend {
    Claude { key: String, model: String },
    DeepInfra { key: String, model: String },
}

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
    /// Claude's content blocks, as it sent them.
    #[serde(skip)]
    blocks: Option<Vec<Value>>,
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
        if let Some(blocks) = &self.blocks {
            m["claude"] = json!(blocks);
        }
        m
    }
}

/// Hears the reply's text as it's written.
pub type OnText<'a> = &'a mut (dyn FnMut(&str) + Send);

/// One chat turn. `tools` are the tools the conversation knows; `use_tools` false: the model
/// has to answer in text. `on_text` hears the text as it comes: Claude's piece by piece, while
/// it writes; DeepInfra's all at once.
pub async fn chat(
    http: &reqwest::Client,
    backend: &Backend,
    messages: &[Value],
    tools: &[Value],
    use_tools: bool,
    on_text: OnText<'_>,
) -> Result<Message> {
    match backend {
        Backend::Claude { key, model } => claude_chat(http, key, model, messages, tools, use_tools, on_text).await,
        Backend::DeepInfra { key, model } => {
            let tools = if use_tools { tools } else { &[] };
            let msg = deepinfra_chat(http, key, model, messages, tools).await?;
            let text = msg.text();
            if !text.is_empty() {
                on_text(&text);
            }
            Ok(msg)
        }
    }
}

async fn deepinfra_chat(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    messages: &[Value],
    tools: &[Value],
) -> Result<Message> {
    let messages: Vec<Value> = messages
        .iter()
        .map(|m| {
            let mut m = m.clone();
            if let Some(o) = m.as_object_mut() {
                o.remove("claude");
            }
            m
        })
        .collect();
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

#[derive(Deserialize)]
struct ClaudeResponse {
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default)]
    stop_reason: Option<String>,
}

impl ClaudeResponse {
    fn into_message(self) -> Message {
        if self.stop_reason.as_deref() == Some("refusal") {
            return Message {
                content: Some("Sorry, I can't help with that.".into()),
                tool_calls: None,
                blocks: None,
            };
        }
        let text: Vec<&str> = self
            .content
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        let calls: Vec<ToolCall> = self
            .content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| ToolCall {
                id: b["id"].as_str().unwrap_or_default().to_string(),
                kind: function_type(),
                function: FunctionCall {
                    name: b["name"].as_str().unwrap_or_default().to_string(),
                    arguments: b["input"].to_string(),
                },
            })
            .collect();
        Message {
            content: Some(text.join("\n")),
            tool_calls: Some(calls).filter(|c| !c.is_empty()),
            blocks: Some(self.content),
        }
    }
}

/// Claude's reply put together from its stream of events, block by block.
#[derive(Default)]
struct ClaudeStream {
    blocks: Vec<Value>,
    /// tool_use blocks' input, as JSON text still arriving.
    inputs: Vec<Option<String>>,
    stop_reason: Option<String>,
}

impl ClaudeStream {
    /// One server-sent event (its lines, without the blank line ending it).
    fn event(&mut self, event: &str, on_text: OnText<'_>) -> Result<()> {
        let data: String = event
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| d.strip_prefix(' ').unwrap_or(d))
            .collect();
        if data.is_empty() {
            return Ok(());
        }
        let e: Value = serde_json::from_str(&data)?;
        let index = e["index"].as_u64().unwrap_or_default() as usize;
        match e["type"].as_str().unwrap_or_default() {
            "content_block_start" => {
                if self.blocks.len() <= index {
                    self.blocks.resize(index + 1, Value::Null);
                    self.inputs.resize(index + 1, None);
                }
                let block = e["content_block"].clone();
                if block["type"] == "tool_use" {
                    self.inputs[index] = Some(String::new());
                }
                if let Some(t) = block["text"].as_str().filter(|t| !t.is_empty()) {
                    on_text(t);
                }
                self.blocks[index] = block;
            }
            "content_block_delta" => {
                let Some(block) = self.blocks.get_mut(index) else { return Ok(()) };
                let delta = &e["delta"];
                let append = |block: &mut Value, field: &str, more: &str| {
                    let now = block[field].as_str().unwrap_or_default().to_string();
                    block[field] = json!(now + more);
                };
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = delta["text"].as_str().unwrap_or_default();
                        append(block, "text", t);
                        on_text(t);
                    }
                    "thinking_delta" => append(block, "thinking", delta["thinking"].as_str().unwrap_or_default()),
                    "signature_delta" => block["signature"] = delta["signature"].clone(),
                    "input_json_delta" => {
                        if let Some(Some(input)) = self.inputs.get_mut(index) {
                            input.push_str(delta["partial_json"].as_str().unwrap_or_default());
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(Some(input)) = self.inputs.get_mut(index).map(Option::take) {
                    let input = serde_json::from_str::<Value>(&input).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
                    self.blocks[index]["input"] = input;
                }
            }
            "message_delta" => {
                if let Some(r) = e["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(r.to_string());
                }
            }
            "error" => {
                let detail = e["error"]["message"].as_str().unwrap_or("the reply broke off");
                return Err(Error::msg(format!("Claude: {detail}")));
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> ClaudeResponse {
        let mut content: Vec<Value> = self.blocks.into_iter().filter(|b| !b.is_null()).collect();
        // A model that declined partway: what it thought or called before the switch doesn't go
        // back (its text does), the switch marker stays where it is.
        if let Some(last) = content.iter().rposition(|b| b["type"] == "fallback") {
            let mut i = 0;
            content.retain(|b| {
                let keep = i >= last
                    || !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking" | "tool_use" | "server_tool_use"));
                i += 1;
                keep
            });
        }
        ClaudeResponse { content, stop_reason: self.stop_reason }
    }
}

async fn claude_chat(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    messages: &[Value],
    tools: &[Value],
    use_tools: bool,
    on_text: OnText<'_>,
) -> Result<Message> {
    let (system, messages) = to_claude(messages);
    let mut body = json!({
        "model": model,
        "max_tokens": CLAUDE_MAX_TOKENS,
        // Streamed, so the voice starts on the first sentence while the rest is written.
        "stream": true,
        "messages": messages,
        // Brief thinking, skipped on simple requests: replies are read aloud, so the time to
        // the first word matters more than depth.
        "thinking": { "type": "adaptive", "block_binding": { "prefix_mismatch_behavior": "drop_block" } },
        "output_config": { "effort": "low" },
        "fallbacks": "default",
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !tools.is_empty() {
        // Always the same list, so it stays cached; past tool calls need it declared anyway.
        body["tools"] = json!(claude_tools(tools));
        if !use_tools {
            body["tool_choice"] = json!({ "type": "none" });
        }
    }
    let resp = http
        .post(CLAUDE_API)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", CLAUDE_BETAS)
        .timeout(TIMEOUT)
        .json(&body)
        .send()
        .await?;
    let mut resp = checked_as(resp, "Claude").await?;
    let mut stream = ClaudeStream::default();
    let mut pending: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        pending.extend(chunk.iter().filter(|&&b| b != b'\r'));
        // Events end with a blank line; one can arrive split across chunks.
        while let Some(end) = pending.windows(2).position(|w| w == b"\n\n") {
            let event: Vec<u8> = pending.drain(..end + 2).collect();
            stream.event(&String::from_utf8_lossy(&event), on_text)?;
        }
    }
    Ok(stream.finish().into_message())
}

/// OpenAI-shaped messages as Claude's system prompt and messages.
fn to_claude(messages: &[Value]) -> (String, Vec<Value>) {
    let mut system = String::new();
    let mut out: Vec<Value> = Vec::new();
    for m in messages {
        let text = m["content"].as_str().unwrap_or_default();
        match m["role"].as_str().unwrap_or_default() {
            "system" => {
                if !system.is_empty() {
                    system.push_str("\n\n");
                }
                system.push_str(text);
            }
            "user" if !text.trim().is_empty() => push(&mut out, "user", vec![json!({ "type": "text", "text": text })]),
            "assistant" => {
                let blocks = match m.get("claude") {
                    Some(Value::Array(b)) if !b.is_empty() => b.clone(),
                    // Said by DeepInfra, or by the launcher itself (clock::quick).
                    _ => {
                        let mut b = Vec::new();
                        if !text.trim().is_empty() {
                            b.push(json!({ "type": "text", "text": text }));
                        }
                        for call in m["tool_calls"].as_array().into_iter().flatten() {
                            let input = call["function"]["arguments"]
                                .as_str()
                                .and_then(|a| serde_json::from_str::<Value>(a).ok())
                                .filter(Value::is_object)
                                .unwrap_or_else(|| json!({}));
                            b.push(json!({
                                "type": "tool_use",
                                "id": tool_id(&call["id"]),
                                "name": call["function"]["name"],
                                "input": input,
                            }));
                        }
                        b
                    }
                };
                if !blocks.is_empty() {
                    push(&mut out, "assistant", blocks);
                }
            }
            "tool" => {
                let content = if text.is_empty() { "(empty)" } else { text };
                push(
                    &mut out,
                    "user",
                    vec![json!({ "type": "tool_result", "tool_use_id": tool_id(&m["tool_call_id"]), "content": content })],
                );
            }
            _ => {}
        }
    }
    (system, out)
}

/// Adds blocks to the conversation, in the last message when it has the same role: all the
/// results of one turn's tool calls go back together.
fn push(out: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if let Some(last) = out.last_mut().filter(|l| l["role"] == role) {
        if let Some(content) = last["content"].as_array_mut() {
            content.extend(blocks);
            return;
        }
    }
    out.push(json!({ "role": role, "content": blocks }));
}

/// A tool call id Claude accepts: letters, digits, `_` and `-`.
fn tool_id(id: &Value) -> String {
    let id: String = id
        .as_str()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.is_empty() {
        "call".into()
    } else {
        id
    }
}

/// OpenAI-shaped tool definitions as Claude's, the list cached up to its end.
fn claude_tools(tools: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = tools
        .iter()
        .map(|t| {
            let f = &t["function"];
            json!({
                "name": f["name"],
                "description": f["description"].as_str().unwrap_or_default(),
                "input_schema": f["parameters"],
            })
        })
        .collect();
    if let Some(last) = out.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
    out
}

/// The response when it succeeded, else an error carrying DeepInfra's own message.
pub async fn checked(resp: reqwest::Response) -> Result<reqwest::Response> {
    checked_as(resp, "DeepInfra").await
}

/// The response when it succeeded, else an error carrying the service's own message.
async fn checked_as(resp: reqwest::Response, service: &str) -> Result<reqwest::Response> {
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
        format!(" (check the {service} API key)")
    } else {
        String::new()
    };
    let detail: String = detail.chars().take(200).collect();
    Err(Error::msg(format!("{service} {status}{hint}: {detail}")))
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
    use std::time::Instant;

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
        assert!(h.get("claude").is_none());

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

    #[test]
    fn claude_tool_use_round_trips() {
        let r: ClaudeResponse = serde_json::from_str(
            r#"{"content":[
                {"type":"thinking","thinking":"","signature":"sig"},
                {"type":"text","text":"Ok."},
                {"type":"tool_use","id":"toolu_1","name":"set_volume","input":{"level":30}}
            ],"stop_reason":"tool_use"}"#,
        )
        .unwrap();
        let m = r.into_message();
        assert_eq!(m.text(), "Ok.");
        assert_eq!(m.tool_calls()[0].function.name, "set_volume");
        assert_eq!(serde_json::from_str::<Value>(&m.tool_calls()[0].function.arguments).unwrap()["level"], 30);

        let history = vec![
            json!({ "role": "system", "content": "Be brief." }),
            json!({ "role": "user", "content": "volume 30" }),
            m.to_history(),
            json!({ "role": "tool", "tool_call_id": "toolu_1", "content": "{\"ok\":true}" }),
        ];
        let (system, msgs) = to_claude(&history);
        assert_eq!(system, "Be brief.");
        assert_eq!(msgs.len(), 3);
        // Claude's blocks go back as it sent them, thinking included.
        assert_eq!(msgs[1]["content"][0]["type"], "thinking");
        assert_eq!(msgs[1]["content"][0]["signature"], "sig");
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[2]["content"][0]["tool_use_id"], "toolu_1");
    }

    fn run_stream(events: &[Value]) -> (Message, String) {
        let mut stream = ClaudeStream::default();
        let mut heard = String::new();
        for e in events {
            let sse = format!("event: {}\ndata: {e}\n", e["type"].as_str().unwrap());
            stream.event(&sse, &mut |t: &str| heard.push_str(t)).unwrap();
        }
        (stream.finish().into_message(), heard)
    }

    #[test]
    fn claude_stream_builds_the_blocks() {
        let (m, heard) = run_stream(&[
            json!({"type":"message_start","message":{"content":[]}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Ligando "}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"as luzes."}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"set_power","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"on\":"}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"true}"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            json!({"type":"message_stop"}),
        ]);
        assert_eq!(heard, "Ligando as luzes.");
        assert_eq!(m.text(), "Ligando as luzes.");
        assert_eq!(m.tool_calls()[0].function.name, "set_power");
        assert_eq!(serde_json::from_str::<Value>(&m.tool_calls()[0].function.arguments).unwrap(), json!({"on": true}));
        let h = m.to_history();
        assert_eq!(h["claude"][0]["signature"], "sig");
        assert_eq!(h["claude"][2]["input"]["on"], true);
    }

    #[test]
    fn claude_stream_drops_what_came_before_a_fallback() {
        let (m, _) = run_stream(&[
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":"a"}}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"Parte"}}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"fallback","from":{"model":"x"},"to":{"model":"y"}}}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"thinking","thinking":"","signature":"b"}}),
            json!({"type":"content_block_start","index":4,"content_block":{"type":"text","text":" final."}}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
        ]);
        let kinds: Vec<&str> = m.blocks.as_ref().unwrap().iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["text", "fallback", "thinking", "text"]);
    }

    /// A real tool round trip with Claude, streamed (costs a fraction of a cent):
    /// `ANTHROPIC_API_KEY=… cargo test -p tvl-server real_claude -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_claude() {
        let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY");
        let key = key.trim();
        let backend = Backend::Claude { key: key.to_string(), model: CLAUDE_MODEL.to_string() };
        let tools = vec![json!({"type":"function","function":{"name":"set_volume","description":"Set the PC volume, 0-100.",
            "parameters":{"type":"object","properties":{"level":{"type":"integer"}},"required":["level"]}}})];
        let http = reqwest::Client::new();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let mut history = vec![
                json!({"role":"system","content":"You are a TV assistant. Reply in one or two short sentences, in the user's language."}),
                json!({"role":"user","content":"Coloca o volume em 30"}),
            ];
            let t = Instant::now();
            let msg = chat(&http, &backend, &history, &tools, true, &mut |_| {}).await.unwrap();
            println!("round 1 in {:?}: tool calls {:?}", t.elapsed(), msg.tool_calls());
            let call = msg.tool_calls()[0].clone();
            assert_eq!(serde_json::from_str::<Value>(&call.function.arguments).unwrap()["level"], 30);
            history.push(msg.to_history());
            history.push(json!({"role":"tool","tool_call_id":call.id,"content":"{\"ok\":true,\"level\":30}"}));

            let t = Instant::now();
            let mut pieces: Vec<(Duration, String)> = Vec::new();
            let msg = chat(&http, &backend, &history, &tools, false, &mut |p| pieces.push((t.elapsed(), p.to_string())))
                .await
                .unwrap();
            println!("round 2 in {:?}: {:?}", t.elapsed(), msg.text());
            println!("  first text after {:?}, {} pieces", pieces.first().map(|p| p.0), pieces.len());
            assert!(!msg.text().is_empty());
            assert_eq!(pieces.iter().map(|p| p.1.as_str()).collect::<String>(), msg.text());
        });
    }

    #[test]
    fn claude_stream_error_event_fails() {
        let mut stream = ClaudeStream::default();
        let e = stream.event("event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"Overloaded\"}}\n", &mut |_| {});
        assert!(e.unwrap_err().to_string().contains("Overloaded"));
    }

    #[test]
    fn openai_turns_become_claude_blocks() {
        let history = vec![
            json!({ "role": "user", "content": "luzes" }),
            json!({ "role": "assistant", "content": "", "tool_calls": [
                {"id":"call.1","type":"function","function":{"name":"a","arguments":"{\"x\":1}"}},
                {"id":"call_2","type":"function","function":{"name":"b","arguments":""}}
            ]}),
            json!({ "role": "tool", "tool_call_id": "call.1", "content": "ok" }),
            json!({ "role": "tool", "tool_call_id": "call_2", "content": "" }),
            json!({ "role": "assistant", "content": "Pronto." }),
            json!({ "role": "user", "content": "valeu" }),
        ];
        let (_, msgs) = to_claude(&history);
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[1]["content"][0]["id"], "call1");
        assert_eq!(msgs[1]["content"][0]["input"]["x"], 1);
        assert_eq!(msgs[1]["content"][1]["input"], json!({}));
        // Both results in one message.
        assert_eq!(msgs[2]["content"].as_array().unwrap().len(), 2);
        assert_eq!(msgs[2]["content"][1]["content"], "(empty)");
        assert_eq!(msgs[3]["role"], "assistant");
        assert_eq!(msgs[4]["content"][0]["text"], "valeu");

        let tools = claude_tools(&[json!({"type":"function","function":{"name":"a","description":"A","parameters":{"type":"object"}}})]);
        assert_eq!(tools[0]["input_schema"]["type"], "object");
        assert_eq!(tools[0]["cache_control"]["type"], "ephemeral");
    }
}
