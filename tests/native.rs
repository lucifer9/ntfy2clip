#![cfg(target_os = "macos")]
use ntfy2clip::{
    clipboard::{Clipboard, Snapshot},
    platform::NativeClipboard,
};
use std::time::Duration;

#[tokio::test]
#[ignore = "overwrites desktop clipboard with synthetic text; run explicitly in a desktop session"]
async fn native_helper_preserves_unicode_and_empty_text() {
    let executable = std::path::PathBuf::from(env!("CARGO_BIN_EXE_n2c"));
    let mut backend = NativeClipboard::new(executable);
    backend.connect().await.unwrap();
    for text in [" n2c synthetic 中文🙂\t\n", ""] {
        backend.write(text, Duration::from_secs(1)).await.unwrap();
        assert!(
            backend.read().await.unwrap() == Snapshot::Text(text.into()),
            "native synthetic readback mismatch (contents withheld)"
        );
    }
    backend.shutdown().await.unwrap();
}
