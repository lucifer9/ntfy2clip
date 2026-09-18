use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    config::Config,
    sync::Coordinator,
};
use std::time::Duration;

struct Desktop {
    snapshot: Snapshot,
    writes: Vec<String>,
    failures: usize,
}
impl Clipboard for Desktop {
    async fn read(&mut self) -> anyhow::Result<Snapshot> {
        Ok(self.snapshot.clone())
    }
    async fn write(&mut self, text: &str, _: Duration) -> Result<(), WriteError> {
        if self.failures > 0 {
            self.failures -= 1;
            return Err(WriteError::Temporary("busy".into()));
        }
        self.snapshot = Snapshot::Text(text.into());
        self.writes.push(text.into());
        Ok(())
    }
}
#[tokio::test(start_paused = true)]
async fn publish_retry_keeps_same_envelope_and_server_cooldown_survives_expiry() {
    let mut cfg = config();
    cfg.send.ttl = Duration::from_secs(3);
    let desktop = Desktop {
        snapshot: Snapshot::Empty,
        writes: vec![],
        failures: 0,
    };
    let mut peer = Coordinator::new(cfg, desktop).unwrap();
    peer.observe().await;
    peer.clipboard_mut().snapshot = Snapshot::Text("X".into());
    peer.observe().await;
    let first = peer.next_publish().unwrap();
    peer.published(ntfy2clip::sync::PublishResult::Retry { after: None });
    assert!(peer.next_publish().is_none());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(peer.next_publish().unwrap().body, first.body);
    peer.published(ntfy2clip::sync::PublishResult::Retry {
        after: Some(Duration::from_secs(5)),
    });
    tokio::time::advance(Duration::from_secs(3)).await;
    peer.clipboard_mut().snapshot = Snapshot::Text("Y".into());
    peer.observe().await;
    assert!(peer.next_publish().is_none());
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(peer.next_publish().unwrap().body.contains("Y"));
}

#[tokio::test(start_paused = true)]
async fn temporary_write_retries_but_new_local_state_cancels_old_target() {
    let desktop = Desktop {
        snapshot: Snapshot::Empty,
        writes: vec![],
        failures: 1,
    };
    let mut peer = Coordinator::new(config(), desktop).unwrap();
    peer.observe().await;
    peer.receive(r#"{"event":"message","topic":"test","message":"remote"}"#)
        .await
        .unwrap();
    peer.write_next().await;
    tokio::time::advance(Duration::from_millis(50)).await;
    peer.write_next().await;
    assert_eq!(peer.clipboard_mut().writes, ["remote"]);
    peer.clipboard_mut().failures = 1;
    peer.receive(r#"{"event":"message","topic":"test","message":"old target"}"#)
        .await
        .unwrap();
    peer.write_next().await;
    peer.clipboard_mut().snapshot = Snapshot::Text("new local".into());
    tokio::time::advance(Duration::from_millis(50)).await;
    peer.write_next().await;
    assert_eq!(peer.clipboard_mut().writes, ["remote"]);
    assert!(peer.next_publish().unwrap().body.contains("new local"));
}

fn config() -> Config {
    Config::parse(|key| match key {
        "TOPIC" => Some("test".into()),
        "SYNC_MODE" => Some("bidirectional".into()),
        _ => None,
    })
    .unwrap()
}
#[tokio::test]
async fn baseline_then_local_change_and_remote_write_do_not_loop() {
    let desktop = Desktop {
        snapshot: Snapshot::Text("old".into()),
        writes: vec![],
        failures: 0,
    };
    let mut peer = Coordinator::new(config(), desktop).unwrap();
    peer.observe().await;
    assert!(peer.next_publish().is_none());
    peer.clipboard_mut().snapshot = Snapshot::Text(" X\r\n".into());
    peer.observe().await;
    let published = peer.next_publish().unwrap();
    assert!(published.body.contains(" X"));
    peer.published(ntfy2clip::sync::PublishResult::Accepted);
    peer.receive(r#"{"event":"message","topic":"test","message":"remote\n"}"#)
        .await
        .unwrap();
    peer.write_next().await;
    peer.observe().await;
    assert_eq!(peer.clipboard_mut().writes, ["remote"]);
    assert!(peer.next_publish().is_none());
}
