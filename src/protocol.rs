use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const TAG: &str = "ntfy2clip";
pub fn normalize(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}

#[derive(Clone)]
pub struct Protocol {
    topic: String,
    origin: Uuid,
    max: usize,
}
#[derive(Deserialize)]
struct Notification {
    event: String,
    topic: String,
    message: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    attachment: Option<serde_json::Value>,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u32,
    origin: Uuid,
    text: String,
}
impl Protocol {
    pub fn new(topic: &str, origin: &str, max: usize) -> Result<Self> {
        Ok(Self {
            topic: topic.into(),
            origin: Uuid::parse_str(origin)?,
            max,
        })
    }
    pub fn encode(&self, text: &str) -> Result<String> {
        let body = serde_json::to_string(&Envelope {
            v: 1,
            origin: self.origin,
            text: normalize(text).into(),
        })?;
        if body.len() > self.max {
            bail!("serialized message exceeds MAX_MESSAGE_BYTES");
        }
        Ok(body)
    }
    pub fn frame_limit(&self) -> usize {
        self.max.saturating_mul(6).saturating_add(65536)
    }
    pub fn decode(&self, frame: &str) -> Result<Option<String>> {
        if frame.len() > self.frame_limit() {
            bail!("ntfy frame too large");
        }
        let msg: Notification = serde_json::from_str(frame)
            .map_err(|_| anyhow::anyhow!("invalid ntfy notification"))?;
        if msg.topic != self.topic || msg.event != "message" {
            return Ok(None);
        }
        if msg.attachment.is_some() {
            bail!("attachments unsupported");
        }
        let Some(body) = msg.message else {
            bail!("ntfy message has no text");
        };
        if body.len() > self.max {
            bail!("received body exceeds MAX_MESSAGE_BYTES");
        }
        let text = if msg.tags.iter().any(|tag| tag == TAG) {
            let envelope: Envelope = serde_json::from_str(&body)
                .map_err(|_| anyhow::anyhow!("invalid application envelope"))?;
            if envelope.v != 1 {
                bail!("unsupported application version");
            }
            if envelope.origin == self.origin {
                return Ok(None);
            }
            envelope.text
        } else {
            body
        };
        if text.len() > self.max {
            bail!("received text exceeds MAX_MESSAGE_BYTES");
        }
        Ok(Some(normalize(&text).into()))
    }
}
