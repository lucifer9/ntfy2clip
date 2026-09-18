use ntfy2clip::{
    config::Config,
    sync::{Publication, PublishResult},
    transport::Publisher,
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn http_publish_sends_tagged_body_and_classifies_failures() {
    for (status, header) in [
        (200, ""),
        (401, ""),
        (429, "Retry-After: 12\r\n"),
        (503, ""),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = listener.local_addr().unwrap().to_string();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            loop {
                let mut chunk = [0; 1024];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.ends_with(b"synthetic-body") {
                    break;
                }
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.starts_with("POST /test HTTP/1.1\r\n"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("x-tags: ntfy2clip\r\n")
            );
            socket.write_all(format!("HTTP/1.1 {status} Test\r\n{header}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        });
        let cfg = Config::parse(|key| match key {
            "SERVER" => Some(server.clone()),
            "SCHEME" => Some("ws".into()),
            "TOPIC" => Some("test".into()),
            _ => None,
        })
        .unwrap();
        let outcome = Publisher::new(cfg)
            .unwrap()
            .publish(Publication {
                body: "synthetic-body".into(),
                timeout: Duration::from_secs(1),
            })
            .await;
        match status {
            200 => assert!(matches!(outcome, PublishResult::Accepted)),
            401 => assert!(matches!(outcome, PublishResult::Rejected)),
            429 => assert!(
                matches!(outcome,PublishResult::Retry { after:Some(delay) } if delay==Duration::from_secs(12))
            ),
            _ => assert!(matches!(outcome, PublishResult::Retry { after: None })),
        }
        task.await.unwrap();
    }
}
