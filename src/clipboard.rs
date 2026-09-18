use anyhow::{Result, anyhow};
use std::time::Duration;
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Snapshot {
    Text(String),
    Empty,
    NonText,
    Unavailable,
}

#[derive(Debug)]
pub enum WriteError {
    Temporary(String),
    Permanent(String),
}
impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Temporary(reason) | Self::Permanent(reason) => f.write_str(reason),
        }
    }
}
impl std::error::Error for WriteError {}

#[allow(async_fn_in_trait)]
pub trait Clipboard {
    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
    async fn read(&mut self) -> Result<Snapshot>;
    /// Returns only after the write completed, or its process was terminated and reaped.
    async fn write(&mut self, text: &str, timeout: Duration) -> Result<(), WriteError>;
}

pub struct CommandClipboard;
impl Clipboard for CommandClipboard {
    async fn read(&mut self) -> Result<Snapshot> {
        Ok(Snapshot::Unavailable)
    }
    async fn write(&mut self, text: &str, timeout: Duration) -> Result<(), WriteError> {
        let mut command = copy_command().map_err(|e| WriteError::Permanent(e.to_string()))?;
        write_command(&mut command, text.as_bytes(), timeout).await
    }
}

fn is_wsl() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft")))
}

pub(crate) fn ensure_supported() -> Result<()> {
    if cfg!(target_os = "windows") || is_wsl() {
        return Err(anyhow!("Windows and WSL are not supported"));
    }
    Ok(())
}

pub fn copy_command() -> Result<Command> {
    ensure_supported()?;
    if cfg!(target_os = "macos") {
        return Ok(Command::new("/usr/bin/pbcopy"));
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Ok(Command::new("wl-copy"));
    }
    if std::env::var_os("DISPLAY").is_some() {
        let mut command = Command::new("xclip");
        command.args(["-selection", "clipboard", "-r", "-in"]);
        return Ok(command);
    }
    Err(anyhow!("no supported clipboard session"))
}

pub async fn write_command(
    command: &mut Command,
    bytes: &[u8],
    timeout: Duration,
) -> Result<(), WriteError> {
    use std::process::Stdio;
    // Child output is untrusted: utilities may echo input; report status, never stderr.
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| WriteError::Permanent(format!("clipboard spawn: {:?}", e.kind())))?;
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("clipboard stdin missing"))?;
        stdin.write_all(bytes).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let status = child.wait().await?;
        if !status.success() {
            return Err(anyhow!("clipboard command exited with {status}"));
        }
        Ok::<(), anyhow::Error>(())
    };
    match tokio::time::timeout(timeout, operation).await {
        Ok(Ok(())) => Ok(()),
        outcome => {
            // Even a broken stdin can leave a child alive. Do not permit a late write.
            if child.try_wait().ok().flatten().is_none() {
                child.kill().await.map_err(|e| {
                    WriteError::Permanent(format!("clipboard termination: {:?}", e.kind()))
                })?;
            }
            child
                .wait()
                .await
                .map_err(|e| WriteError::Permanent(format!("clipboard reap: {:?}", e.kind())))?;
            match outcome {
                Ok(Err(e)) => Err(WriteError::Temporary(format!(
                    "clipboard write failed: {e}"
                ))),
                _ => Err(WriteError::Temporary(
                    "clipboard write timed out; process reaped".into(),
                )),
            }
        }
    }
}
