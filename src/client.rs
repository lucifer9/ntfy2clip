use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
#[cfg(target_os = "macos")]
use oslog::OsLogger;
use serde::Deserialize;
use std::env;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
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
#[cfg(target_os = "macos")]
fn create_clip_command() -> Result<(&'static str, &'static str, Command)> {
    Ok(("pbcopy", "macOS", Command::new("/usr/bin/pbcopy")))
}

#[cfg(not(target_os = "macos"))]
fn create_clip_command() -> Result<(&'static str, &'static str, Command)> {
    match env::consts::FAMILY {
        "unix" => {
            if env::var("WSL_DISTRO_NAME").is_ok() {
                Ok((
                    "clip.exe",
                    "WSL",
                    Command::new("/mnt/c/Windows/System32/clip.exe"),
                ))
            } else if env::var("WAYLAND_DISPLAY").is_ok() {
                Ok(("wl-copy", "Wayland", Command::new("/usr/bin/wl-copy")))
            } else if env::var("DISPLAY").is_ok() {
                let mut cmd = Command::new("/usr/bin/xclip");
                cmd.args(["-sel", "clip", "-r", "-in"]);
                Ok(("xclip", "Xorg", cmd))
            } else {
                Err(anyhow!("Unsupported Unix environment"))
            }
        }
        "windows" => Ok(("clip.exe", "Windows", Command::new("clip.exe"))),
        _ => Err(anyhow!("Unsupported operating system")),
    }
}

async fn spawn_clip_process(mut cmd: Command) -> Result<Child> {
    use std::process::Stdio;
    cmd.stdin(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn clipboard process: {}", e))
}

async fn set_clip(content: String) -> Result<()> {
    info!("Setting clipboard to: {}", &content);

    let (copy_command, cur_env, cmd) = create_clip_command()?;
    debug!(
        "Running under {}, using copy command {}",
        cur_env, copy_command
    );

    let mut child = spawn_clip_process(cmd).await?;
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Failed to open stdin"))?;

    child_stdin.write_all(content.as_bytes()).await?;
    child_stdin.flush().await?;
    drop(child_stdin);
    child.wait().await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    let dev = env::var("DEV").is_ok();
    let log_level = if dev {
        log::LevelFilter::Debug
    } else {
        env::var("RUST_LOG")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(log::LevelFilter::Info)
    };

    #[cfg(not(target_os = "macos"))]
    pretty_env_logger::formatted_builder()
        .filter_level(log_level)
        .init();

    #[cfg(target_os = "macos")]
    OsLogger::new("ntfyclip")
        .level_filter(log_level)
        .category_level_filter("Settings", log::LevelFilter::Trace)
        .init()
        .expect("Failed to initialize logger");
    #[cfg(target_os = "macos")]
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .unwrap();

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

const DEFAULT_TIMEOUT: u64 = 120;

async fn connect_and_run() -> Result<()> {
    let timeout = env::var("TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&t| t > 0)
        .unwrap_or(DEFAULT_TIMEOUT);
    let server = env::var("SERVER").unwrap_or_else(|_| "ntfy.sh".to_string());
    let scheme = env::var("SCHEME").unwrap_or_else(|_| "wss".to_string());
    let topic = env::var("TOPIC").map_err(|_| anyhow!("TOPIC environment variable is required"))?;
    let url = Url::parse(&format!("{}://{}/{}/ws", scheme, server, topic))
        .map_err(|e| anyhow!("Invalid URL: {}", e))?;
    let token = env::var("TOKEN").unwrap_or_default();
    let mut request = url
        .into_client_request()
        .map_err(|e| anyhow!("Failed to create request: {}", e))?;
    if !token.is_empty() {
        let auth_value = format!("Bearer {token}")
            .parse()
            .map_err(|_| anyhow!("Invalid token format"))?;
        request.headers_mut().insert("Authorization", auth_value);
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
                                        spawn(async move {
                                            if let Err(e) = set_clip(message).await {
                                                error!("Failed to set clipboard: {}", e);
                                            }
                                        });
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
                    return Err(anyhow!("No traffic in the last {} seconds", timeout));
                }
            }
        }
    }
}
