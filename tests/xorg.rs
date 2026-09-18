#![cfg(target_os = "linux")]
use ntfy2clip::{
    clipboard::{Clipboard, Snapshot},
    config::Config,
    platform::LinuxClipboard,
    sync::Coordinator,
};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};
#[tokio::test]
#[ignore = "requires isolated X server DISPLAY; overwrites only that server's selections"]
async fn xfixes_clipboard_keeps_selection_alive_and_ignores_primary() {
    let mut backend = LinuxClipboard::new(env!("CARGO_BIN_EXE_n2c").into(), true).unwrap();
    for text in [" Xorg 中文🙂 \n", ""] {
        backend.write(text, Duration::from_secs(1)).await.unwrap();
        assert_eq!(backend.read().await.unwrap(), Snapshot::Text(text.into()));
    }
    backend
        .write("stable", Duration::from_secs(1))
        .await
        .unwrap();
    let config = Config::parse(|key| match key {
        "TOPIC" => Some("test".into()),
        "SYNC_MODE" => Some("bidirectional".into()),
        _ => None,
    })
    .unwrap();
    let mut peer = Coordinator::new(config, backend).unwrap();
    peer.observe().await;
    let mut primary = Command::new("xclip")
        .args(["-selection", "primary", "-quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = primary.stdin.take().unwrap();
    input.write_all(b"PRIMARY is not CLIPBOARD").await.unwrap();
    drop(input);
    tokio::time::sleep(Duration::from_millis(50)).await;
    peer.observe().await;
    assert!(peer.next_publish().is_none());
    peer.receive(r#"{"topic":"test","event":"message","message":"from network\r\n"}"#)
        .await
        .unwrap();
    peer.write_next().await;
    peer.observe().await;
    assert_eq!(
        peer.clipboard_mut().read().await.unwrap(),
        Snapshot::Text("from network".into())
    );
    assert!(peer.next_publish().is_none());
    primary.kill().await.unwrap();
    primary.wait().await.unwrap();
    peer.clipboard_mut().shutdown().await.unwrap();
}
