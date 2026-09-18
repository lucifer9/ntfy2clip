use anyhow::{Context, Result, bail};
use std::time::Duration;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Receive,
    Bidirectional,
}
#[derive(Clone)]
pub struct Budget {
    pub messages: usize,
    pub bytes: usize,
    pub ttl: Duration,
    pub timeout: Duration,
    pub attempts: u32,
    pub retry_base: Duration,
    pub retry_max: Duration,
}
#[derive(Clone)]
pub struct Config {
    pub mode: Mode,
    pub topic: String,
    pub token: String,
    pub ws_url: Url,
    pub http_url: Url,
    pub traffic_timeout: Duration,
    pub poll: Duration,
    pub max_message: usize,
    pub send: Budget,
    pub write: Budget,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        Self::parse(|key| std::env::var(key).ok())
    }
    pub fn parse(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let number = |name: &str, default: u64| -> Result<u64> {
            let n = match get(name) {
                Some(value) => value
                    .parse::<u64>()
                    .with_context(|| format!("invalid {name}"))?,
                None => default,
            };
            if n == 0 || n > u32::MAX as u64 {
                bail!("{name} must be in 1..=4294967295");
            }
            Ok(n)
        };
        let mode = match get("SYNC_MODE").as_deref().unwrap_or("receive") {
            "receive" => Mode::Receive,
            "bidirectional" => Mode::Bidirectional,
            _ => bail!("SYNC_MODE must be receive or bidirectional"),
        };
        let topic = get("TOPIC").context("TOPIC environment variable is required")?;
        if topic.is_empty()
            || topic
                .chars()
                .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        {
            bail!("TOPIC must be one alphanumeric, underscore or hyphen topic");
        }
        let scheme = get("SCHEME").unwrap_or_else(|| "wss".into());
        let http_scheme = match scheme.as_str() {
            "wss" => "https",
            "ws" => "http",
            _ => bail!("SCHEME must be ws or wss"),
        };
        let server = get("SERVER").unwrap_or_else(|| "ntfy.sh".into());
        let mut ws_url = Url::parse(&format!("{scheme}://{server}"))
            .map_err(|_| anyhow::anyhow!("invalid SERVER"))?;
        if !ws_url.username().is_empty()
            || ws_url.password().is_some()
            || ws_url.query().is_some()
            || ws_url.fragment().is_some()
            || ws_url.path() != "/"
        {
            bail!("SERVER must contain only host and optional port");
        }
        ws_url.set_path(&format!("/{topic}/ws"));
        let mut http_url = ws_url.clone();
        http_url
            .set_scheme(http_scheme)
            .map_err(|_| anyhow::anyhow!("invalid HTTP scheme"))?;
        http_url.set_path(&format!("/{topic}"));
        let send = Budget {
            messages: number("SEND_QUEUE_MAX_MESSAGES", 128)? as usize,
            bytes: number("SEND_QUEUE_MAX_BYTES", 8388608)? as usize,
            ttl: Duration::from_secs(number("SEND_TTL_SECS", 60)?),
            timeout: Duration::from_secs(number("PUBLISH_TIMEOUT_SECS", 10)?),
            attempts: number("PUBLISH_MAX_ATTEMPTS", 5)? as u32,
            retry_base: Duration::from_millis(number("PUBLISH_RETRY_BASE_MS", 1000)?),
            retry_max: Duration::from_millis(number("PUBLISH_RETRY_MAX_MS", 8000)?),
        };
        let write = Budget {
            messages: number("RECEIVE_QUEUE_MAX_MESSAGES", 128)? as usize,
            bytes: number("RECEIVE_QUEUE_MAX_BYTES", 8388608)? as usize,
            ttl: Duration::from_millis(number("WRITE_TTL_MS", 5000)?),
            timeout: Duration::from_millis(number("WRITE_TIMEOUT_MS", 1000)?),
            attempts: number("WRITE_MAX_ATTEMPTS", 5)? as u32,
            retry_base: Duration::from_millis(number("WRITE_RETRY_BASE_MS", 50)?),
            retry_max: Duration::from_millis(number("WRITE_RETRY_MAX_MS", 400)?),
        };
        if send.retry_base > send.retry_max || write.retry_base > write.retry_max {
            bail!("retry base exceeds retry maximum");
        }
        Ok(Self {
            mode,
            topic,
            token: get("TOKEN").unwrap_or_default(),
            ws_url,
            http_url,
            traffic_timeout: Duration::from_secs(
                get("TIMEOUT")
                    .and_then(|s| s.parse().ok())
                    .filter(|v| *v > 0)
                    .unwrap_or(120),
            ),
            poll: Duration::from_millis(number("CLIPBOARD_POLL_MS", 250)?),
            max_message: number("MAX_MESSAGE_BYTES", 4096)? as usize,
            send,
            write,
        })
    }
}
