use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    config::Config,
    sync::Coordinator,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;
struct Desktop {
    state: Arc<Mutex<Snapshot>>,
    writes: Arc<Mutex<Vec<String>>>,
    started: Arc<Notify>,
    finish: Arc<Notify>,
    delayed: bool,
}
impl Clipboard for Desktop {
    async fn read(&mut self) -> anyhow::Result<Snapshot> {
        Ok(self.state.lock().unwrap().clone())
    }
    async fn write(&mut self, text: &str, _: Duration) -> Result<(), WriteError> {
        *self.state.lock().unwrap() = Snapshot::Text(text.into());
        self.writes.lock().unwrap().push(text.into());
        if !self.delayed {
            self.delayed = true;
            self.started.notify_one();
            self.finish.notified().await;
        }
        Ok(())
    }
}
#[tokio::test]
async fn early_write_notification_and_concurrent_ingress_keep_one_accounted_writer() {
    let started = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let writes = Arc::new(Mutex::new(vec![]));
    let desktop = Desktop {
        state: Arc::new(Mutex::new(Snapshot::Empty)),
        writes: writes.clone(),
        started: started.clone(),
        finish: finish.clone(),
        delayed: false,
    };
    let config = Config::parse(|key| match key {
        "TOPIC" => Some("test".into()),
        "SYNC_MODE" => Some("bidirectional".into()),
        "RECEIVE_QUEUE_MAX_MESSAGES" => Some("2".into()),
        _ => None,
    })
    .unwrap();
    let mut peer = Coordinator::new(config, desktop).unwrap();
    peer.observe().await;
    let receiver = peer.receiver();
    receiver
        .receive(r#"{"topic":"test","event":"message","message":"head"}"#)
        .unwrap();
    let task = tokio::spawn(async move {
        peer.write_next().await;
        peer
    });
    started.notified().await;
    receiver
        .receive(r#"{"topic":"test","event":"message","message":"old waiting"}"#)
        .unwrap();
    receiver
        .receive(r#"{"topic":"test","event":"message","message":"new waiting"}"#)
        .unwrap();
    finish.notify_one();
    let mut peer = task.await.unwrap();
    peer.observe().await;
    assert!(peer.next_publish().is_none());
    peer.write_next().await;
    peer.observe().await;
    assert!(peer.next_publish().is_none());
    assert_eq!(*writes.lock().unwrap(), ["head", "new waiting"]);
}
