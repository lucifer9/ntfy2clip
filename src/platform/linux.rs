use super::NativeClipboard;
use crate::{
    clipboard::{Clipboard, Snapshot, WriteError},
    ipc::{MAX_FRAME, Operation, ResponseResult},
};
use anyhow::{Result, bail};
use std::{io::Read, process::Stdio, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    time::Instant,
};
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{
        Event,
        xfixes::{ConnectionExt as _, SelectionEventMask},
        xproto::{ConnectionExt as _, *},
    },
    rust_connection::RustConnection,
};

pub enum Reader {
    Wayland,
    Xorg(Box<XReader>),
}
impl Reader {
    pub fn new() -> Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            use wl_clipboard_rs::paste::{self, ClipboardType, Error, Seat};
            match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
                Ok(_) | Err(Error::ClipboardEmpty) | Err(Error::NoMimeType) => {}
                Err(_) => bail!("data-control protocol or seat unavailable"),
            }
            Ok(Self::Wayland)
        } else {
            Ok(Self::Xorg(Box::new(XReader::new()?)))
        }
    }
    pub fn operate(&mut self, operation: Operation) -> ResponseResult {
        if !matches!(operation, Operation::Read) {
            return ResponseResult::Permanent;
        }
        let result = match self {
            Self::Wayland => wayland_read(),
            Self::Xorg(reader) => reader.read(),
        };
        match result {
            Ok(value) => ResponseResult::Snapshot { value, revision: 0 },
            Err(_) => ResponseResult::Temporary,
        }
    }
}
fn wayland_once() -> Result<Snapshot> {
    use wl_clipboard_rs::paste::{self, ClipboardType, Error, MimeType, Seat};
    match paste::get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text) {
        Ok((pipe, _)) => {
            let mut bytes = vec![];
            pipe.take(MAX_FRAME as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > MAX_FRAME {
                bail!("clipboard text exceeds IPC bound");
            }
            Ok(Snapshot::Text(String::from_utf8(bytes).map_err(|_| {
                anyhow::anyhow!("clipboard text is not UTF-8")
            })?))
        }
        Err(Error::ClipboardEmpty) => Ok(Snapshot::Empty),
        Err(Error::NoMimeType) => Ok(Snapshot::NonText),
        Err(_) => bail!("data-control clipboard unavailable"),
    }
}
fn wayland_read() -> Result<Snapshot> {
    // Fresh data-control offers on each request, never queued payloads. Check a
    // second fresh offer after transfer to reject changes during pipe delivery.
    let first = wayland_once()?;
    if wayland_once()? != first {
        bail!("selection changed during transfer");
    }
    Ok(first)
}

pub struct XReader {
    connection: RustConnection,
    window: Window,
    clipboard: Atom,
    property: Atom,
    targets: Atom,
    utf8: Atom,
    changed: std::cell::Cell<bool>,
}
impl XReader {
    fn new() -> Result<Self> {
        let (connection, screen) = x11rb::connect(None)?;
        let observe = std::env::args().any(|arg| arg == "--observe");
        if observe {
            connection.xfixes_query_version(5, 0)?.reply()?;
        }
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id()?;
        connection
            .create_window(
                0,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?
            .check()?;
        let atom = |name: &[u8]| -> Result<Atom> {
            Ok(connection.intern_atom(false, name)?.reply()?.atom)
        };
        let clipboard = atom(b"CLIPBOARD")?;
        let property = atom(b"N2C_TRANSFER")?;
        let targets = atom(b"TARGETS")?;
        let utf8 = atom(b"UTF8_STRING")?;
        if observe {
            connection
                .xfixes_select_selection_input(
                    window,
                    clipboard,
                    SelectionEventMask::SET_SELECTION_OWNER
                        | SelectionEventMask::SELECTION_WINDOW_DESTROY
                        | SelectionEventMask::SELECTION_CLIENT_CLOSE,
                )?
                .check()?;
        }
        connection.flush()?;
        Ok(Self {
            connection,
            window,
            clipboard,
            property,
            targets,
            utf8,
            changed: std::cell::Cell::new(false),
        })
    }
    fn convert(&self, target: Atom) -> Result<GetPropertyReply> {
        self.connection
            .delete_property(self.window, self.property)?;
        self.connection.convert_selection(
            self.window,
            self.clipboard,
            target,
            self.property,
            CURRENT_TIME,
        )?;
        self.connection.flush()?;
        loop {
            let event = self.connection.wait_for_event()?;
            if matches!(event, Event::XfixesSelectionNotify(_)) {
                self.changed.set(true);
            }
            if let Event::SelectionNotify(event) = event {
                if event.requestor != self.window || event.selection != self.clipboard {
                    continue;
                }
                if event.property == NONE {
                    bail!("selection conversion refused");
                }
                let reply = self
                    .connection
                    .get_property(
                        true,
                        self.window,
                        self.property,
                        AtomEnum::ANY,
                        0,
                        (MAX_FRAME / 4) as u32,
                    )?
                    .reply()?;
                if reply.bytes_after != 0 {
                    bail!("selection exceeds transfer limit");
                }
                if reply.type_ == self.connection.intern_atom(false, b"INCR")?.reply()?.atom {
                    return self.incremental();
                }
                return Ok(reply);
            }
        }
    }
    fn incremental(&self) -> Result<GetPropertyReply> {
        let mut bytes = Vec::new();
        self.connection.flush()?;
        loop {
            let event = self.connection.wait_for_event()?;
            if matches!(event, Event::XfixesSelectionNotify(_)) {
                self.changed.set(true);
            }
            if let Event::PropertyNotify(event) = event {
                if event.window != self.window
                    || event.atom != self.property
                    || event.state != Property::NEW_VALUE
                {
                    continue;
                }
                let mut chunk = self
                    .connection
                    .get_property(
                        true,
                        self.window,
                        self.property,
                        AtomEnum::ANY,
                        0,
                        (MAX_FRAME / 4) as u32,
                    )?
                    .reply()?;
                if chunk.bytes_after != 0 || bytes.len() + chunk.value.len() > MAX_FRAME {
                    bail!("incremental selection exceeds transfer limit");
                }
                if chunk.value.is_empty() {
                    chunk.value = bytes;
                    chunk.value_len =
                        (chunk.value.len() / (chunk.format.max(8) as usize / 8)) as u32;
                    return Ok(chunk);
                }
                bytes.extend_from_slice(&chunk.value);
                self.connection.flush()?;
            }
        }
    }
    fn read(&self) -> Result<Snapshot> {
        while self.connection.poll_for_event()?.is_some() {}
        self.changed.set(false);
        let owner = self
            .connection
            .get_selection_owner(self.clipboard)?
            .reply()?
            .owner;
        if owner == NONE {
            return Ok(Snapshot::Empty);
        }
        let targets = self.convert(self.targets)?;
        let atoms: Vec<_> = targets
            .value32()
            .ok_or_else(|| anyhow::anyhow!("invalid selection targets"))?
            .collect();
        let value = if atoms.contains(&self.utf8) {
            Snapshot::Text(
                String::from_utf8(self.convert(self.utf8)?.value)
                    .map_err(|_| anyhow::anyhow!("selection is not UTF-8"))?,
            )
        } else if atoms.contains(&u32::from(AtomEnum::STRING)) {
            Snapshot::Text(
                self.convert(AtomEnum::STRING.into())?
                    .value
                    .into_iter()
                    .map(char::from)
                    .collect(),
            )
        } else {
            Snapshot::NonText
        };
        if owner
            != self
                .connection
                .get_selection_owner(self.clipboard)?
                .reply()?
                .owner
        {
            bail!("selection changed during transfer");
        }
        while let Some(event) = self.connection.poll_for_event()? {
            if matches!(event, Event::XfixesSelectionNotify(_)) {
                bail!("selection changed during transfer");
            }
        }
        if self.changed.get() {
            bail!("selection changed during transfer");
        }
        Ok(value)
    }
}

pub struct LinuxClipboard {
    reader: NativeClipboard,
    owner: Option<Child>,
    wayland: bool,
}
impl LinuxClipboard {
    pub fn new(executable: std::path::PathBuf, observe: bool) -> Result<Self> {
        Ok(Self {
            reader: NativeClipboard::new(executable).with_observation(observe),
            owner: None,
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
        })
    }
    pub async fn connect(&mut self) -> Result<()> {
        self.reader.connect().await
    }
    async fn stop_owner(&mut self) -> Result<()> {
        if let Some(mut owner) = self.owner.take() {
            if owner.try_wait()?.is_none() {
                owner.kill().await?;
            }
            let status = owner.wait().await?;
            log::debug!("selection owner reaped status={status}");
        }
        Ok(())
    }
}
impl Clipboard for LinuxClipboard {
    async fn read(&mut self) -> Result<Snapshot> {
        if let Some(owner) = &mut self.owner
            && let Some(status) = owner.try_wait()?
        {
            if !status.success() {
                log::warn!("selection owner exited unsuccessfully");
            }
            self.owner = None;
        }
        self.reader.read().await
    }
    async fn shutdown(&mut self) -> Result<()> {
        self.stop_owner().await?;
        self.reader.shutdown().await
    }
    async fn write(&mut self, text: &str, timeout: Duration) -> Result<(), WriteError> {
        self.stop_owner()
            .await
            .map_err(|_| WriteError::Permanent("selection owner termination failed".into()))?;
        let mut command = if self.wayland {
            let mut c = Command::new("wl-copy");
            c.args(["--foreground", "--type", "text/plain;charset=utf-8"]);
            c
        } else {
            let mut c = Command::new("xclip");
            c.args([
                "-selection",
                "clipboard",
                "-in",
                "-quiet",
                "-target",
                "UTF8_STRING",
            ]);
            c
        };
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| WriteError::Permanent("selection owner spawn failed".into()))?;
        self.owner = Some(child);
        let deadline = Instant::now() + timeout;
        let operation = async {
            let mut input = self.owner.as_mut().unwrap().stdin.take().unwrap();
            input.write_all(text.as_bytes()).await?;
            input.shutdown().await?;
            drop(input);
            loop {
                if self
                    .owner
                    .as_mut()
                    .unwrap()
                    .try_wait()?
                    .is_some_and(|status| !status.success())
                {
                    bail!("selection owner failed");
                }
                match self
                    .reader
                    .request(
                        Operation::Read,
                        deadline.saturating_duration_since(Instant::now()),
                    )
                    .await?
                {
                    ResponseResult::Snapshot {
                        value: Snapshot::Text(actual),
                        ..
                    } if actual == text => return Ok::<(), anyhow::Error>(()),
                    _ => {}
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        match tokio::time::timeout_at(deadline, operation).await {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.stop_owner().await.map_err(|_| {
                    WriteError::Permanent("selection owner termination failed".into())
                })?;
                self.reader.stop().await.map_err(|_| {
                    WriteError::Permanent("selection reader termination failed".into())
                })?;
                Err(WriteError::Temporary(
                    "selection write failed or timed out; processes reaped".into(),
                ))
            }
        }
    }
}
