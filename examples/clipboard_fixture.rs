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
            if cfg!(target_os = "linux") && !ntfy2clip::clipboard::is_wsl() {
                CommandClipboard
                    .write(&text, Duration::from_secs(1))
                    .await?;
            } else {
                clipboard.write(&text, Duration::from_secs(1)).await?;
            }
        }
        "assert" => {
            if clipboard.read().await? != Snapshot::Text(text) {
                clipboard.shutdown().await?;
                bail!("synthetic clipboard assertion failed (contents withheld)");
            }
        }
        _ => bail!("expected set/assert"),
    }
    clipboard.shutdown().await?;
    println!("synthetic clipboard {operation} passed");
    Ok(())
}
