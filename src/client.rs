use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use serde::Deserialize;
use std::io::Cursor;
use std::process::{Command, Stdio};
use std::{env, io};
use tokio::spawn;
use tokio::time::{self, Duration, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

#[derive(Deserialize, Debug)]
struct WSMessage {
    event: String,
    topic: String,
    message: Option<String>,
}

async fn set_clip(content: String) -> Result<()> {
    info!("Setting clipboard to: {}", &content);
    let mut child = Command::new("/usr/bin/wl-copy")
        .stdin(Stdio::piped())
        .spawn()?;
    let child_stdin = child.stdin.as_mut().unwrap();
    let mut cursor = Cursor::new(content.as_bytes());
    io::copy(&mut cursor, child_stdin)?;
    child.wait()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let dev = env::var("DEV").is_ok();
    if dev {
        env::set_var("RUST_LOG", "debug");
    }
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();
    loop {
        match connect_and_run().await {
            Ok(()) => println!("Connection closed cleanly"),
            Err(e) => {
                error!("Connection error: {:?}. Reconnecting...", e);
                // Optionally add a delay before reconnecting
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn connect_and_run() -> Result<()> {
    let timeout = env::var("TIMEOUT")
        .unwrap_or("120".to_string())
        .parse::<u64>()
        .unwrap();
    let server = env::var("SERVER").unwrap_or("ntfy.sh".to_string());
    let scheme = env::var("SHEME").unwrap_or("wss".to_string());
    let topic = env::var("TOPIC").expect("You must subscribe to a topic.");
    let url =
        Url::parse(format!("{}://{}/{}/ws", scheme, server, topic).as_str()).expect("Invalid URL");
    let token = env::var("TOKEN").unwrap_or("".to_string());
    let mut request = url.into_client_request().unwrap();
    if !token.is_empty() {
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    }

    debug!("request: {:?}", &request);
    let (mut ws_stream, _) = connect_async(request).await?;
    info!("connected to {server} with topic={topic} and timeout={timeout}");

    let mut ping_interval = time::interval(Duration::from_secs(timeout));
    let mut last_traffic = Instant::now();

    loop {
        tokio::select! {
            Some(msg) = ws_stream.next() => {
                last_traffic = Instant::now();
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<WSMessage>(&text) {
                            Ok(msg) => {
                                if (msg.topic == topic) && (msg.event == "message") {
                                    debug!("WS received message: {:?}", &msg);
                                    if let Some(message) = msg.message {
                                        spawn(set_clip(message));
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Error in WebSocket connection: {}", e);
                            }
                        }
                    }
                    Ok(Message::Ping(ping)) => {
                        ws_stream.send(Message::Pong(ping)).await?;
                        debug!("WS received ping and sent pong");
                    }
                    Ok(Message::Pong(_)) => {
                        debug!("WS received pong");
                    }
                    Ok(Message::Close(_)) => {
                        debug!("WS received close message");
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow!(e.to_string()));
                    }
                    _ => {}
                }
            },
            _ = ping_interval.tick() => {
                if last_traffic.elapsed() > Duration::from_secs(timeout) {
                    return Err(anyhow!("No traffic in the last 120 seconds".to_string()));
                }
            }
        }
    }
}
