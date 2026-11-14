//! A reply read out on the TV PC that asked while the model is still writing it: each sentence goes
//! there as soon as it's finished ("assistant.say"), in order, and "assistant.said" ends the reply.
//! The TV PC's voice starts on the first sentence instead of after the whole reply.

use serde_json::json;
use tokio::sync::mpsc;

use crate::ctx::Ctx;

/// How long a TV PC gets to take one sentence.
const SAY_TIMEOUT_MS: u32 = 10_000;

/// The reply being read out on a TV PC. Dropping it ends the reply there.
pub struct Live {
    tx: mpsc::UnboundedSender<String>,
}

impl Live {
    /// Starts reading reply `id` out on TV PC `device`.
    pub fn start(app: &Ctx, device: &str, id: u32) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let (app, device) = (app.clone(), device.to_string());
        tokio::spawn(async move {
            // One at a time, so the sentences arrive in order.
            while let Some(text) = rx.recv().await {
                let said = json!({ "id": id, "text": text });
                if let Err(e) = app.agents.call(&device, "assistant.say", said, None, SAY_TIMEOUT_MS, false).await {
                    tracing::warn!("{device} didn't take a sentence of the reply: {e:?}");
                }
            }
            let _ = app.agents.call(&device, "assistant.said", json!({ "id": id }), None, SAY_TIMEOUT_MS, false).await;
        });
        Self { tx }
    }

    pub fn say(&self, text: &str) {
        let _ = self.tx.send(text.to_string());
    }
}

/// Cuts text arriving in pieces into whole sentences, for the voice to start on the first one
/// before the rest is written.
#[derive(Default)]
pub struct Sentences {
    rest: String,
    any: bool,
}

impl Sentences {
    /// Adds a piece; returns the sentences it finished.
    pub fn push(&mut self, piece: &str) -> Vec<String> {
        self.rest.push_str(piece);
        let mut out = Vec::new();
        loop {
            let mut chars = self.rest.char_indices().peekable();
            let mut cut = None;
            while let Some((i, c)) = chars.next() {
                let ends = match c {
                    '\n' => true,
                    // "24.5" and "R$ 2,50" go on; ".", "!" or "?" before a space ends one.
                    '.' | '!' | '?' | '…' | ':' | ';' => chars.peek().is_some_and(|(_, n)| n.is_whitespace()),
                    _ => false,
                };
                if ends {
                    cut = Some(i + c.len_utf8());
                    break;
                }
            }
            let Some(cut) = cut else { break };
            let sentence = self.rest[..cut].trim().to_string();
            self.rest.drain(..cut);
            if !sentence.is_empty() {
                self.any = true;
                out.push(sentence);
            }
        }
        out
    }

    /// What's left once the text is complete.
    pub fn flush(&mut self) -> Option<String> {
        let last = std::mem::take(&mut self.rest).trim().to_string();
        if last.is_empty() {
            return None;
        }
        self.any = true;
        Some(last)
    }

    /// Whether any sentence came out.
    pub fn any(&self) -> bool {
        self.any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_come_out_as_they_end() {
        let mut s = Sentences::default();
        assert!(s.push("Pronto, liguei").is_empty());
        assert!(!s.any());
        assert_eq!(s.push(" as luzes. Amanhã faz 24.5 graus"), ["Pronto, liguei as luzes."]);
        // The end of a piece may still be a decimal point or an abbreviation.
        assert!(s.push(" e custa R$ 2.").is_empty());
        assert_eq!(s.push("50! Quer mais?\nSim"), ["Amanhã faz 24.5 graus e custa R$ 2.50!", "Quer mais?"]);
        assert_eq!(s.flush().as_deref(), Some("Sim"));
        assert_eq!(s.flush(), None);
        assert!(s.any());
    }
}
