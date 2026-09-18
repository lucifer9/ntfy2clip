use crate::{
    config::Config,
    protocol::TAG,
    sync::{Publication, PublishResult, Receiver},
};
use anyhow::{Result, bail};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use std::time::{Duration, SystemTime};
use tokio::time::{self, Instant};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message, WebSocketConfig},
    },
};

#[derive(Clone)]
pub struct Publisher {
    client: Client,
    config: Config,
}
impl Publisher {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            client: Client::builder().redirect(Policy::none()).build()?,
            config,
        })
    }
    pub async fn publish(&self, job: Publication) -> PublishResult {
        let mut request = self
            .client
            .post(self.config.http_url.clone())
            .header("X-Tags", TAG)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(job.body)
            .timeout(job.timeout);
        if !self.config.token.is_empty() {
            request = request.bearer_auth(&self.config.token);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => PublishResult::Accepted,
            Ok(response) if response.status() == StatusCode::TOO_MANY_REQUESTS => {
                let after = response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(retry_after)
                    .unwrap_or(self.config.send.retry_base);
                PublishResult::Retry { after: Some(after) }
            }
            Ok(response)
                if response.status().is_server_error()
                    && response.status() != StatusCode::NOT_IMPLEMENTED
                    && response.status() != StatusCode::HTTP_VERSION_NOT_SUPPORTED =>
            {
                PublishResult::Retry { after: None }
            }
            Ok(response) => {
                log::warn!(
                    "HTTP publish rejected status={}",
                    response.status().as_u16()
                );
                PublishResult::Rejected
            }
            Err(error) if error.is_builder() => PublishResult::Rejected,
            Err(_) => PublishResult::Retry { after: None },
        }
    }
}
fn retry_after(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds.min(u32::MAX as u64)));
    }
    httpdate::parse_http_date(value).ok().map(|date| {
        date.duration_since(SystemTime::now())
            .unwrap_or_default()
            .min(Duration::from_secs(u32::MAX as u64))
    })
}

pub async fn subscribe(config: Config, receiver: Receiver) -> Result<()> {
    loop {
        match connection(&config, &receiver).await {
            Ok(()) => log::warn!("subscription closed; reconnecting"),
            Err(_) => log::warn!("subscription failed; reconnecting"),
        }
        time::sleep(Duration::from_secs(5)).await;
    }
}
async fn connection(config: &Config, receiver: &Receiver) -> Result<()> {
    let mut request = config.ws_url.as_str().into_client_request()?;
    if !config.token.is_empty() {
        let mut auth = format!("Bearer {}", config.token)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid TOKEN header"))?;
        reqwest::header::HeaderValue::set_sensitive(&mut auth, true);
        request.headers_mut().insert("Authorization", auth);
    }
    let limit = crate::protocol::frame_limit(config.max_message);
    let socket_config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    let (mut socket, _) = time::timeout(
        config.traffic_timeout,
        connect_async_with_config(request, Some(socket_config), false),
    )
    .await??;
    log::info!("subscription connected");
    let mut deadline = Instant::now() + config.traffic_timeout;
    loop {
        tokio::select! {
            _ = time::sleep_until(deadline) => bail!("subscription traffic timeout"),
            message = socket.next() => {
                deadline = Instant::now()+config.traffic_timeout;
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(error) = receiver.receive(&text) { log::warn!("receive skipped: {error}"); }
                    }
                    Some(Ok(Message::Ping(bytes))) => time::timeout(config.traffic_timeout,socket.send(Message::Pong(bytes))).await??,
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Err(_)) => bail!("subscription transport error"),
                    _ => {},
                }
            }
        }
    }
}
