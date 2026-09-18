use crate::{
    clipboard::{Clipboard, CommandClipboard, Snapshot, WriteError},
    ipc::{self, Operation, Request, Response, ResponseResult},
};
use anyhow::{Context, Result, bail};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::LinuxClipboard;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub fn helper_main() -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        #[cfg(target_os = "linux")]
        let mut reader = linux::Reader::new()?;
        #[cfg(target_os = "windows")]
        let _listener = if std::env::args().any(|arg| arg == "--observe") {
            Some(windows::Listener::new()?)
        } else {
            None
        };
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stdout().lock();
        loop {
            let request: Request = match ipc::read_frame(&mut input) {
                Ok(request) => request,
                Err(e) if e.is::<ipc::EndOfStream>() => return Ok(()),
                Err(e) => return Err(e),
            };
            if matches!(request.operation, Operation::Capabilities) {
                ipc::write_frame(
                    &mut output,
                    &Response {
                        pid: std::process::id(),
                        version: 1,
                        id: request.id,
                        result: ResponseResult::Ready,
                    },
                )?;
                continue;
            }
            // On WSL, terminating an interop proxy alone is insufficient proof
            // that a Windows native call stopped. The helper enforces its own
            // write deadline, and the parent also terminates its Windows PID.
            let watchdog = if matches!(request.operation, Operation::Write(_)) {
                let (done, wait) = std::sync::mpsc::channel();
                let budget = Duration::from_millis(request.budget_ms.max(1));
                Some((
                    done,
                    std::thread::spawn(move || {
                        if matches!(
                            wait.recv_timeout(budget),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        ) {
                            std::process::exit(2);
                        }
                    }),
                ))
            } else {
                None
            };
            #[cfg(target_os = "macos")]
            let result = macos::operate(request.operation);
            #[cfg(target_os = "windows")]
            let result = windows::operate(request.operation);
            #[cfg(target_os = "linux")]
            let result = reader.operate(request.operation);
            if let Some((done, thread)) = watchdog {
                let _ = done.send(());
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("helper watchdog panicked"))?;
            }
            ipc::write_frame(
                &mut output,
                &Response {
                    pid: std::process::id(),
                    version: 1,
                    id: request.id,
                    result,
                },
            )?;
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    bail!("native helper requires macOS or Windows")
}

pub struct NativeClipboard {
    executable: PathBuf,
    child: Option<Child>,
    id: u64,
    native_pid: Option<u32>,
    poisoned: bool,
    observe: bool,
}
impl NativeClipboard {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            child: None,
            id: 0,
            native_pid: None,
            poisoned: false,
            observe: false,
        }
    }
    pub fn with_observation(mut self, enabled: bool) -> Self {
        self.observe = enabled;
        self
    }
    pub async fn connect(&mut self) -> Result<()> {
        match self
            .request(Operation::Capabilities, Duration::from_secs(10))
            .await?
        {
            ResponseResult::Ready => Ok(()),
            _ => bail!("helper capability handshake failed"),
        }
    }
    async fn stop(&mut self) -> Result<()> {
        self.poisoned = true;
        if let Some(child) = self.child.as_mut() {
            // EOF allows the native listener to unregister on its owning thread.
            child.stdin.take();
            if let Ok(status) = tokio::time::timeout(Duration::from_millis(200), child.wait()).await
            {
                status.context("helper graceful reap failed")?;
                self.child = None;
                self.native_pid = None;
                self.poisoned = false;
                return Ok(());
            }
        }
        if let Some(child) = self.child.as_mut()
            && child.try_wait()?.is_some()
        {
            self.child = None;
            self.native_pid = None;
            self.poisoned = false;
            return Ok(());
        }
        if crate::clipboard::is_wsl()
            && self
                .executable
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            && let Some(pid) = self.native_pid
        {
            let mut killer = Command::new("/mnt/c/Windows/System32/taskkill.exe");
            killer
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            if crate::clipboard::write_command(&mut killer, b"", Duration::from_secs(5))
                .await
                .is_err()
                && self
                    .child
                    .as_mut()
                    .is_some_and(|child| child.try_wait().ok().flatten().is_none())
            {
                bail!("Windows helper termination failed; backend disabled");
            }
        }
        if let Some(mut child) = self.child.take() {
            if child.try_wait()?.is_none() {
                child.kill().await?;
            }
            child.wait().await?;
        }
        self.native_pid = None;
        self.poisoned = false;
        Ok(())
    }
    async fn request(&mut self, operation: Operation, timeout: Duration) -> Result<ResponseResult> {
        if self.poisoned {
            bail!("helper backend disabled after termination failure");
        }
        self.id = self.id.wrapping_add(1);
        let bytes = ipc::encode(&Request {
            id: self.id,
            operation,
            budget_ms: timeout.as_millis().min(u64::MAX as u128) as u64,
        })?;
        if self.child.is_none() {
            self.child = Some(
                Command::new(&self.executable)
                    .arg("--clipboard-helper")
                    .args(if self.observe {
                        vec!["--observe"]
                    } else {
                        vec![]
                    })
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .context("helper spawn failed")?,
            );
        }
        let child = self.child.as_mut().unwrap();
        let operation = async {
            let stdin = child.stdin.as_mut().context("helper stdin unavailable")?;
            stdin.write_all(&bytes).await?;
            stdin.flush().await?;
            let stdout = child.stdout.as_mut().context("helper stdout unavailable")?;
            let size = stdout.read_u32().await? as usize;
            if size == 0 || size > ipc::MAX_FRAME {
                bail!("invalid helper frame length");
            }
            let mut body = vec![0; size];
            stdout.read_exact(&mut body).await?;
            let response: Response = serde_json::from_slice(&body)
                .map_err(|_| anyhow::anyhow!("invalid helper frame"))?;
            if response.version != 1 || response.id != self.id {
                bail!("helper protocol or request mismatch");
            }
            self.native_pid = Some(response.pid);
            Ok(response.result)
        };
        match tokio::time::timeout(timeout, operation).await {
            Ok(Ok(result)) => Ok(result),
            outcome => {
                let reason = match outcome {
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => "deadline exceeded".into(),
                    Ok(Ok(_)) => unreachable!(),
                };
                self.stop()
                    .await
                    .context("helper termination/reap failed")?;
                bail!(
                    "helper IPC failed: {reason}; terminated and reaped; next operation starts a fresh helper"
                )
            }
        }
    }
}
impl Clipboard for NativeClipboard {
    async fn shutdown(&mut self) -> Result<()> {
        self.stop().await
    }
    async fn read(&mut self) -> Result<Snapshot> {
        match self
            .request(Operation::Read, Duration::from_secs(1))
            .await?
        {
            ResponseResult::Snapshot { value, .. } => Ok(value),
            ResponseResult::TemporaryReason(reason) => bail!("native clipboard read: {reason}"),
            _ => bail!("native clipboard read unavailable"),
        }
    }
    async fn write(&mut self, text: &str, timeout: Duration) -> Result<(), WriteError> {
        match self.request(Operation::Write(text.into()), timeout).await {
            Ok(ResponseResult::Written) => Ok(()),
            Ok(ResponseResult::Permanent) => Err(WriteError::Permanent(
                "native clipboard cannot represent text".into(),
            )),
            Ok(ResponseResult::TemporaryReason(reason)) => Err(WriteError::Temporary(reason)),
            Err(error) => Err(WriteError::Temporary(error.to_string())),
            _ => Err(WriteError::Temporary(
                "native clipboard operation failed".into(),
            )),
        }
    }
}

pub enum Backend {
    Native(Box<NativeClipboard>),
    Command(CommandClipboard),
    #[cfg(target_os = "linux")]
    Linux(Box<linux::LinuxClipboard>),
}
impl Backend {
    pub async fn detect(observe: bool) -> Result<(Self, bool)> {
        let native = if cfg!(any(target_os = "macos", target_os = "windows")) {
            Some(std::env::current_exe()?)
        } else if crate::clipboard::is_wsl() {
            // WSL must never fall through to WSLg's selection.
            Some(
                std::env::var_os("N2C_WINDOWS_HELPER")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("n2c.exe")),
            )
        } else {
            None
        };
        if let Some(path) = native {
            let mut native = NativeClipboard::new(path).with_observation(observe);
            match native.connect().await {
                Ok(_) => return Ok((Self::Native(Box::new(native)), true)),
                Err(_) => {
                    log::warn!(
                        "native helper unavailable; receive-only command fallback, current clipboard cannot be read"
                    );
                    native.stop().await?;
                }
            }
        }
        #[cfg(target_os = "linux")]
        if !crate::clipboard::is_wsl()
            && (std::env::var_os("WAYLAND_DISPLAY").is_some()
                || std::env::var_os("DISPLAY").is_some())
        {
            let gnome = std::env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("gnome")
                && std::env::var_os("WAYLAND_DISPLAY").is_some();
            if !gnome {
                let mut backend = linux::LinuxClipboard::new(std::env::current_exe()?, observe)?;
                match backend.connect().await {
                    Ok(_) => return Ok((Self::Linux(Box::new(backend)), true)),
                    Err(_) => {
                        log::warn!(
                            "Linux observation unavailable: check data-control/seat or XFixes/display; retaining command receive path"
                        );
                        backend.shutdown().await?;
                    }
                }
            } else {
                log::warn!("GNOME Wayland automatic upload unsupported; command receive path only");
            }
        }
        Ok((Self::Command(CommandClipboard), false))
    }
}
impl Clipboard for Backend {
    async fn shutdown(&mut self) -> Result<()> {
        match self {
            Self::Native(backend) => backend.shutdown().await,
            Self::Command(backend) => backend.shutdown().await,
            #[cfg(target_os = "linux")]
            Self::Linux(backend) => backend.shutdown().await,
        }
    }
    async fn read(&mut self) -> Result<Snapshot> {
        match self {
            Self::Native(backend) => backend.read().await,
            Self::Command(backend) => backend.read().await,
            #[cfg(target_os = "linux")]
            Self::Linux(backend) => backend.read().await,
        }
    }
    async fn write(&mut self, text: &str, timeout: Duration) -> Result<(), WriteError> {
        match self {
            Self::Native(backend) => backend.write(text, timeout).await,
            Self::Command(backend) => backend.write(text, timeout).await,
            #[cfg(target_os = "linux")]
            Self::Linux(backend) => backend.write(text, timeout).await,
        }
    }
}
