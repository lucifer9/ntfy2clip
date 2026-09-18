//! Synthetic desktop-test fixture. Never prints clipboard contents.
use anyhow::{Result, bail};
use ntfy2clip::{
    clipboard::{Clipboard, CommandClipboard, Snapshot},
    platform::NativeClipboard,
};
use std::{path::PathBuf, time::Duration};
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let executable = PathBuf::from(args.next().expect("n2c helper path"));
    let operation = args.next().expect("set/assert");
    let text = args.next().expect("synthetic text");
    let mut clipboard = NativeClipboard::new(executable);
    clipboard.connect().await?;
    match operation.as_str() {
        "set" => {
            if cfg!(target_os = "linux") {
                CommandClipboard
                    .write(&text, Duration::from_secs(1))
                    .await?;
            } else {
                clipboard.write(&text, Duration::from_secs(1)).await?;
            }
        }
        "assert" => {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let actual = clipboard.read().await?;
                if actual == Snapshot::Text(text.clone()) {
                    break;
                }
                if tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    continue;
                }
                clipboard.shutdown().await?;
                bail!("synthetic clipboard assertion timed out (contents withheld)");
            }
        }
        _ => bail!("expected set/assert"),
    }
    clipboard.shutdown().await?;
    println!("synthetic clipboard {operation} passed");
    Ok(())
}
