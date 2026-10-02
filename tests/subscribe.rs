use futures_util::{SinkExt, StreamExt};
use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    config::Config,
    protocol::frame_limit,
    sync::Coordinator,
    transport,
};
use std::time::Duration;
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};

#[derive(Default)]
struct MemoryClipboard {
    text: Option<String>,
    writes: Vec<String>,
}
impl Clipboard for MemoryClipboard {
    async fn read(&mut self) -> anyhow::Result<Snapshot> {
        Ok(self.text.clone().map_or(Snapshot::Empty, Snapshot::Text))
    }
    async fn write(&mut self, text: &str, _: Duration) -> Result<(), WriteError> {
        self.text = Some(text.into());
        self.writes.push(text.into());
        Ok(())
    }
}

fn config(listener: &TcpListener) -> Config {
    Config::parse(|key| match key {
        "SERVER" => Some(listener.local_addr().unwrap().to_string()),
        "SCHEME" => Some("ws".into()),
        "TOPIC" => Some("test".into()),
        "TOKEN" => Some("synthetic-token".into()),
        _ => None,
    })
    .unwrap()
}

#[allow(
    clippy::result_large_err,
    reason = "tungstenite fixes the handshake callback's error type"
)]
async fn accept(listener: &TcpListener) -> WebSocketStream<TcpStream> {
    let (stream, _) = listener.accept().await.unwrap();
    accept_hdr_async(stream, |request: &Request, response: Response| {
        assert_eq!(request.uri().path(), "/test/ws");
        assert_eq!(request.headers()["authorization"], "Bearer synthetic-token");
        Ok(response)
    })
    .await
    .unwrap()
}

async fn fence(socket: &mut WebSocketStream<TcpStream>) {
    // Pong proves that earlier Text frames have been processed, without sleeps.
    socket
        .send(Message::Ping(b"fence".as_slice().into()))
        .await
        .unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::Pong(b"fence".as_slice().into())
    );
}

#[tokio::test]
async fn subscription_authenticates_preserves_sync_text_and_survives_bad_messages() {
    timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cfg = config(&listener);
        let mut peer = Coordinator::new(cfg.clone(), MemoryClipboard::default()).unwrap();
        let subscriber = tokio::spawn(transport::subscribe(cfg, peer.receiver()));
        let mut socket = accept(&listener).await;
        for text in [
            "not JSON",
            r#"{"event":"keepalive","topic":"test"}"#,
            r#"{"event":"message","topic":"other","message":"wrong topic"}"#,
            r#"{"event":"message","topic":"test","tags":["ntfy2clip"],"message":"{\"v\":2,\"origin\":\"2b1fda12-6da0-4fcb-b124-a7411e2b83cd\",\"text\":\"unsupported\"}"}"#,
            r#"{"event":"message","topic":"test","tags":["ntfy2clip"],"message":"{\"v\":1,\"origin\":\"2b1fda12-6da0-4fcb-b124-a7411e2b83cd\",\"text\":\" 中文🙂 \\r\\n\"}"}"#,
        ] {
            socket.send(Message::Text(text.into())).await.unwrap();
        }
        fence(&mut socket).await;
        for _ in 0..5 { peer.write_next().await; }
        assert_eq!(peer.clipboard_mut().writes, [" 中文🙂 "]);
        subscriber.abort();
        assert!(subscriber.await.unwrap_err().is_cancelled());
    }).await.expect("subscription must process messages and Ping promptly");
}

#[tokio::test]
async fn subscription_reconnects_after_close_and_delivers_new_messages() {
    timeout(Duration::from_secs(12), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cfg = config(&listener);
        let mut peer = Coordinator::new(cfg.clone(), MemoryClipboard::default()).unwrap();
        let subscriber = tokio::spawn(transport::subscribe(cfg, peer.receiver()));
        let mut first = accept(&listener).await;
        first.close(None).await.unwrap();
        drop(first);
        let mut second = accept(&listener).await;
        second
            .send(Message::Text(
                r#"{"event":"message","topic":"test","message":"after reconnect"}"#.into(),
            ))
            .await
            .unwrap();
        fence(&mut second).await;
        peer.write_next().await;
        assert_eq!(peer.clipboard_mut().writes, ["after reconnect"]);
        subscriber.abort();
        assert!(subscriber.await.unwrap_err().is_cancelled());
    })
    .await
    .expect("peer must reconnect after the bounded retry delay");
}

#[tokio::test]
async fn subscription_closes_oversized_frames_and_idle_connections() {
    timeout(Duration::from_secs(5), async {
        for oversized in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut cfg = config(&listener);
            cfg.max_message = 64;
            cfg.traffic_timeout = if oversized {
                Duration::from_secs(30)
            } else {
                Duration::from_millis(100)
            };
            let mut peer = Coordinator::new(cfg.clone(), MemoryClipboard::default()).unwrap();
            let limit = frame_limit(cfg.max_message);
            let subscriber = tokio::spawn(transport::subscribe(cfg, peer.receiver()));
            let mut socket = accept(&listener).await;
            if oversized {
                socket
                    .send(Message::Text("x".repeat(limit + 1).into()))
                    .await
                    .unwrap();
            }
            let closed = socket.next().await;
            assert!(
                matches!(closed, None | Some(Err(_)) | Some(Ok(Message::Close(_)))),
                "unexpected response: {closed:?}"
            );
            peer.write_next().await;
            assert!(peer.clipboard_mut().writes.is_empty());
            subscriber.abort();
            assert!(subscriber.await.unwrap_err().is_cancelled());
        }
    })
    .await
    .expect("oversized frames and idle sockets must be closed");
}
