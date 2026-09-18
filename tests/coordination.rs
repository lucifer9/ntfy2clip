use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    config::Config,
    sync::{Coordinator, PublishResult},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Clone)]
struct Desktop {
    state: Arc<Mutex<Snapshot>>,
    writes: Arc<Mutex<Vec<String>>>,
    fail: usize,
    reads_fail: bool,
    replace_after_write: bool,
}
impl Desktop {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Snapshot::Empty)),
            writes: Default::default(),
            fail: 0,
            reads_fail: false,
            replace_after_write: false,
        }
    }
}
impl Clipboard for Desktop {
    async fn read(&mut self) -> anyhow::Result<Snapshot> {
        if self.reads_fail {
            anyhow::bail!("read failure")
        }
        Ok(self.state.lock().unwrap().clone())
    }
    async fn write(&mut self, text: &str, _: Duration) -> Result<(), WriteError> {
        self.writes.lock().unwrap().push(text.into());
        if self.fail > 0 {
            self.fail -= 1;
            return Err(WriteError::Temporary("busy".into()));
        }
        *self.state.lock().unwrap() = Snapshot::Text(if self.replace_after_write {
            "external".into()
        } else {
            text.into()
        });
        Ok(())
    }
}
#[tokio::test(start_paused = true)]
async fn receive_bytes_include_started_job_and_reject_when_nothing_else_fits() {
    let mut cfg = config();
    cfg.write.bytes = 4;
    let mut desktop = Desktop::new();
    desktop.fail = 1;
    let mut peer = Coordinator::new(cfg, desktop).unwrap();
    peer.observe().await;
    peer.receiver().receive(&frame("a")).unwrap();
    peer.write_next().await;
    peer.receiver().receive(&frame("b")).unwrap();
    peer.receiver().receive(&frame("cccc")).unwrap();
    tokio::time::advance(Duration::from_millis(50)).await;
    peer.write_next().await;
    peer.write_next().await;
    assert_eq!(*peer.clipboard_mut().writes.lock().unwrap(), ["a", "a"]);
}
#[tokio::test]
async fn early_and_delayed_own_origin_never_overwrite_local_content() {
    let mut peer = Coordinator::new(config(), Desktop::new()).unwrap();
    peer.observe().await;
    *peer.clipboard_mut().state.lock().unwrap() = Snapshot::Text("X".into());
    peer.observe().await;
    let body = peer.next_publish().unwrap().body;
    let echo =
        serde_json::json!({"event":"message","topic":"test","tags":["ntfy2clip"],"message":body})
            .to_string();
    peer.receiver().receive(&echo).unwrap();
    peer.write_next().await;
    *peer.clipboard_mut().state.lock().unwrap() = Snapshot::Text("Y".into());
    peer.observe().await;
    peer.published(PublishResult::Accepted);
    peer.receiver().receive(&echo).unwrap();
    peer.write_next().await;
    assert!(peer.clipboard_mut().writes.lock().unwrap().is_empty());
    assert_eq!(
        *peer.clipboard_mut().state.lock().unwrap(),
        Snapshot::Text("Y".into())
    );
}

fn config() -> Config {
    Config::parse(|key| match key {
        "TOPIC" => Some("test".into()),
        "SYNC_MODE" => Some("bidirectional".into()),
        "SEND_QUEUE_MAX_MESSAGES" | "RECEIVE_QUEUE_MAX_MESSAGES" => Some("2".into()),
        _ => None,
    })
    .unwrap()
}
fn frame(text: &str) -> String {
    serde_json::json!({"event":"message","topic":"test","message":text}).to_string()
}
#[tokio::test(start_paused = true)]
async fn current_state_not_history_and_read_failure_is_not_empty() {
    let mut peer = Coordinator::new(config(), Desktop::new()).unwrap();
    peer.observe().await;
    for state in [
        Snapshot::Text("X\r\n".into()),
        Snapshot::Text("X".into()),
        Snapshot::NonText,
        Snapshot::Text("X".into()),
        Snapshot::Text("Y".into()),
        Snapshot::Text("X".into()),
    ] {
        *peer.clipboard_mut().state.lock().unwrap() = state;
        peer.observe().await;
    }
    // Capacity retains the newest observations rather than a historical hash set.
    assert!(peer.next_publish().unwrap().body.contains("Y"));
    peer.published(PublishResult::Accepted);
    assert!(peer.next_publish().unwrap().body.contains("X"));
    peer.published(PublishResult::Accepted);
    peer.clipboard_mut().reads_fail = true;
    peer.observe().await;
    peer.clipboard_mut().reads_fail = false;
    peer.observe().await;
    assert!(peer.next_publish().is_none());
}
#[tokio::test(start_paused = true)]
async fn receive_fifo_accounts_for_started_head_and_evicts_oldest_waiting() {
    let mut desktop = Desktop::new();
    desktop.fail = 1;
    let mut peer = Coordinator::new(config(), desktop).unwrap();
    peer.observe().await;
    let receiver = peer.receiver();
    receiver.receive(&frame("head")).unwrap();
    peer.write_next().await;
    receiver.receive(&frame("old waiting")).unwrap();
    receiver.receive(&frame("new waiting")).unwrap();
    tokio::time::advance(Duration::from_millis(50)).await;
    peer.write_next().await;
    peer.write_next().await;
    assert_eq!(
        *peer.clipboard_mut().writes.lock().unwrap(),
        ["head", "head", "new waiting"]
    );
}
#[tokio::test(start_paused = true)]
async fn failures_stop_at_budget_and_expired_head_does_not_block() {
    let mut desktop = Desktop::new();
    desktop.fail = 99;
    let mut peer = Coordinator::new(config(), desktop).unwrap();
    peer.observe().await;
    peer.receiver().receive(&frame("bounded")).unwrap();
    for delay in [0, 50, 100, 200, 400, 800] {
        tokio::time::advance(Duration::from_millis(delay)).await;
        peer.write_next().await;
    }
    assert_eq!(peer.clipboard_mut().writes.lock().unwrap().len(), 5);
    peer.receiver().receive(&frame("expire")).unwrap();
    tokio::time::advance(Duration::from_secs(5)).await;
    peer.clipboard_mut().fail = 0;
    peer.receiver().receive(&frame("fresh")).unwrap();
    peer.write_next().await;
    assert_eq!(
        peer.clipboard_mut().writes.lock().unwrap().last().unwrap(),
        "fresh"
    );
}
#[tokio::test]
async fn external_overwrite_after_native_completion_is_published_not_swallowed() {
    let mut desktop = Desktop::new();
    desktop.replace_after_write = true;
    let mut peer = Coordinator::new(config(), desktop).unwrap();
    peer.observe().await;
    peer.receiver().receive(&frame("remote")).unwrap();
    peer.write_next().await;
    peer.observe().await;
    assert!(peer.next_publish().unwrap().body.contains("external"));
}
#[tokio::test(start_paused = true)]
async fn send_capacity_includes_inflight_and_lifetime_includes_backoff() {
    let mut peer = Coordinator::new(config(), Desktop::new()).unwrap();
    peer.observe().await;
    *peer.clipboard_mut().state.lock().unwrap() = Snapshot::Text("head".into());
    peer.observe().await;
    let first = peer.next_publish().unwrap();
    for text in ["old", "new"] {
        *peer.clipboard_mut().state.lock().unwrap() = Snapshot::Text(text.into());
        peer.observe().await;
    }
    assert!(peer.next_publish().is_none());
    peer.published(PublishResult::Retry { after: None });
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(peer.next_publish().unwrap().body, first.body);
    peer.published(PublishResult::Accepted);
    assert!(peer.next_publish().unwrap().body.contains("new"));
    peer.published(PublishResult::Rejected);
    assert!(peer.next_publish().is_none());
}
