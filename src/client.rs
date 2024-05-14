use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use serde::Deserialize;
#[cfg(target_os = "windows")]
use std::{ffi::{c_uint, c_void, OsStr}, os::windows::ffi::OsStrExt, ptr, io::Error};
use std::env;
#[cfg(target_family = "unix")]
use std::{io::Cursor, process::{Command, Stdio}};
#[cfg(target_family = "unix")]
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

#[cfg(target_os = "windows")]
#[link(name = "user32", kind = "dylib")]
extern "system" {
    fn OpenClipboard(hWndNewOwner: *mut c_void) -> bool;
    fn SetClipboardData(uformat: c_uint, data: *mut c_void) -> *mut c_void;
    fn CloseClipboard() -> bool;
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32", kind = "dylib")]
extern "system" {
    fn GlobalAlloc(uFlags: c_uint, dwBytes: usize) -> *mut c_void;
    fn GlobalLock(hMem: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(hMem: *mut c_void) -> bool;
}

#[cfg(target_family = "unix")]
async fn set_clip(content: String) -> Result<()> {
    info!("Setting clipboard to: {}", &content);

    #[cfg(target_os = "linux")]
    let cmd: Option<Command> = Some(if env::var("WSL_DISTRO_NAME").is_ok() {
        Command::new(Some("/mnt/c/Windows/System32/clip.exe").unwrap());
    } else if env::var("WAYLAND_DISPLAY").is_ok() {
        Command::new(Some("/usr/bin/wl-copy").unwrap())
    } else if env::var("DISPLAY").is_ok() {
        Command::new(Some("/usr/bin/xclip").unwrap())
            .arg("-sel")
            .arg("clip")
            .arg("-r")
            .arg("-in")
    });

    #[cfg(target_os = "macos")]
    let cmd: Option<Command> = Some(Command::new(Some("/usr/bin/pbcopy").unwrap()));

    let mut child = cmd.unwrap().stdin(Stdio::piped()).spawn()?;
    let child_stdin = child.stdin.as_mut().unwrap();
    let mut cursor = Cursor::new(content.as_bytes());
    io::copy(&mut cursor, child_stdin)?;
    child.wait()?;

    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn set_clip(content: String) -> Result<(), Error> {
    let text: Vec<u16> = OsStr::new(&content)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect();

    let hglob = GlobalAlloc(2 /* GMEM_MOVEABLE */, text.len() * std::mem::size_of::<u16>());

    if hglob == ptr::null_mut() {
        return Err(Error::last_os_error());
    }

    let dst = GlobalLock(hglob);

    if dst == ptr::null_mut() {
        return Err(Error::last_os_error());
    }

    ptr::copy_nonoverlapping(text.as_ptr(), dst as _, text.len());

    GlobalUnlock(hglob);

    if !OpenClipboard(ptr::null_mut()) {
        return Err(Error::last_os_error());
    }

    if SetClipboardData(13 /* CF_UNICODETEXT */ , hglob) == ptr::null_mut() {
        CloseClipboard();
        return Err(Error::last_os_error());
    }

    CloseClipboard();

    Ok(())
}

async fn connect_and_run() -> Result<()> {
    let timeout = env::var("TIMEOUT")
        .unwrap_or("120".to_string())
        .parse::<u64>()
        .unwrap();
    let server = env::var("SERVER").unwrap_or("ntfy.sh".to_string());
    let scheme = env::var("SCHEME").unwrap_or("wss".to_string());
    let topic = env::var("TOPIC").expect("You must subscribe to a topic.");
    let url =
        Url::parse(format!("{}://{}/{}/ws", scheme, server, topic).as_str()).expect("Invalid URL");
    let token = env::var("TOKEN").unwrap_or("".to_string());
    let mut request = url.into_client_request().unwrap();
    if !token.is_empty() {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
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
                                        #[cfg(target_os = "windows")]
                                        unsafe { set_clip(message).unwrap() };
                                        #[cfg(target_family = "unix")]
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
