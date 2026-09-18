use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    config::Config,
    sync::Coordinator,
    transport::{self, Publisher},
};
use std::time::Duration;
struct Desktop {
    value: Snapshot,
    writes: usize,
}
impl Clipboard for Desktop {
    async fn read(&mut self) -> anyhow::Result<Snapshot> {
        Ok(self.value.clone())
    }
    async fn write(&mut self, text: &str, _: Duration) -> Result<(), WriteError> {
        self.value = Snapshot::Text(text.into());
        self.writes += 1;
        Ok(())
    }
}
#[tokio::test]
#[ignore = "requires controlled official ntfy on N2C_TEST_SERVER, synthetic content only"]
async fn official_ntfy_three_subscribers_fan_out_without_echo_writes() {
    let server = std::env::var("N2C_TEST_SERVER").expect("N2C_TEST_SERVER");
    let topic = format!("n2c-test-{}", uuid::Uuid::new_v4());
    let config = Config::parse(|key| match key {
        "SERVER" => Some(server.clone()),
        "SCHEME" => Some("ws".into()),
        "TOPIC" => Some(topic.clone()),
        "SYNC_MODE" => Some("bidirectional".into()),
        _ => None,
    })
    .unwrap();
    let publisher = Publisher::new(config.clone()).unwrap();
    let mut peers = vec![];
    let mut subscriptions = vec![];
    for _ in 0..3 {
        let mut peer = Coordinator::new(
            config.clone(),
            Desktop {
                value: Snapshot::Empty,
                writes: 0,
            },
        )
        .unwrap();
        peer.observe().await;
        subscriptions.push(tokio::spawn(transport::subscribe(
            config.clone(),
            peer.receiver(),
        )));
        peers.push(peer);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut publications = 0;
    for source in 0..3 {
        let text = format!(" synthetic 中文🙂 {source} ");
        peers[source].clipboard_mut().value = Snapshot::Text(format!("{text}\r\n"));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            for peer in &mut peers {
                peer.observe().await;
                peer.write_next().await;
                if let Some(job) = peer.next_publish() {
                    publications += 1;
                    peer.published(publisher.publish(job).await);
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(publications, source + 1);
        for (index, peer) in peers.iter_mut().enumerate() {
            let expected = if index == source {
                format!("{text}\r\n")
            } else {
                text.clone()
            };
            assert_eq!(peer.clipboard_mut().value, Snapshot::Text(expected));
        }
    }
    assert_eq!(
        peers
            .iter_mut()
            .map(|peer| peer.clipboard_mut().writes)
            .collect::<Vec<_>>(),
        vec![2, 2, 2]
    );
    for subscription in subscriptions {
        subscription.abort();
        let _ = subscription.await;
    }
}
