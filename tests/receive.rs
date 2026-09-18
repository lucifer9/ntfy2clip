use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    config::Config,
    sync::Coordinator,
};
#[derive(Default)]
struct Desktop {
    writes: Vec<String>,
    fail: bool,
}
impl Clipboard for Desktop {
    async fn read(&mut self) -> anyhow::Result<Snapshot> {
        Ok(Snapshot::Unavailable)
    }
    async fn write(&mut self, text: &str, _: std::time::Duration) -> Result<(), WriteError> {
        if self.fail {
            return Err(WriteError::Permanent("unsupported".into()));
        }
        self.writes.push(text.into());
        Ok(())
    }
}
#[tokio::test]
async fn production_receiver_filters_and_failure_does_not_block_following_message() {
    let config = Config::parse(|key| (key == "TOPIC").then(|| "test".into())).unwrap();
    let mut peer = Coordinator::new(config, Desktop::default()).unwrap();
    for frame in [
        r#"{"topic":"other","event":"message","message":"ignored"}"#,
        r#"{"topic":"test","event":"open","message":"ignored"}"#,
        r#"{"topic":"test","event":"message","message":"hello\r\n"}"#,
    ] {
        peer.receive(frame).await.unwrap();
        peer.write_next().await;
    }
    assert_eq!(peer.clipboard_mut().writes, ["hello"]);
    peer.clipboard_mut().fail = true;
    peer.receive(r#"{"topic":"test","event":"message","message":"failure"}"#)
        .await
        .unwrap();
    peer.write_next().await;
    peer.clipboard_mut().fail = false;
    peer.receive(r#"{"topic":"test","event":"message","message":"next"}"#)
        .await
        .unwrap();
    peer.write_next().await;
    assert_eq!(peer.clipboard_mut().writes, ["hello", "next"]);
}
