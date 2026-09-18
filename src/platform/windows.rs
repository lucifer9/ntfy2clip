use crate::{
    clipboard::Snapshot,
    ipc::{Operation, ResponseResult},
};
use anyhow::{Result, bail};
use std::{
    ptr::{null, null_mut},
    sync::mpsc,
    thread,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
    UI::WindowsAndMessaging::*,
};
const UNICODE: u32 = 13;
static CHANGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static SNAPSHOT: std::sync::Mutex<Option<(u64, Snapshot)>> = std::sync::Mutex::new(None);
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLIPBOARDUPDATE {
        CHANGED.store(true, std::sync::atomic::Ordering::Release);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

pub struct Listener {
    thread: Option<thread::JoinHandle<()>>,
    id: u32,
}
impl Listener {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || unsafe {
            let class: Vec<u16> = "ntfy2clip-listener\0".encode_utf16().collect();
            let instance = GetModuleHandleW(null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                let _ = tx.send(None);
                return;
            }
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                instance,
                null(),
            );
            if window.is_null() || AddClipboardFormatListener(window) == 0 {
                let _ = tx.send(None);
                return;
            }
            let id = GetWindowThreadProcessId(window, null_mut());
            if tx.send(Some(id)).is_err() {
                RemoveClipboardFormatListener(window);
                DestroyWindow(window);
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            loop {
                match GetMessageW(&mut msg, null_mut(), 0, 0) {
                    0 => break,
                    -1 => std::process::exit(3), // Parent observes EOF and restarts the helper.
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            RemoveClipboardFormatListener(window);
            DestroyWindow(window);
        });
        match rx.recv()? {
            Some(id) => Ok(Self {
                thread: Some(thread),
                id,
            }),
            None => {
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("clipboard listener panicked"))?;
                bail!("clipboard listener unavailable")
            }
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Open;
impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
fn failure(operation: &str) -> ResponseResult {
    ResponseResult::TemporaryReason(format!("{operation} failed code={}", unsafe {
        GetLastError()
    }))
}
pub fn operate(operation: Operation) -> ResponseResult {
    // The process boundary bounds native RPCs; the clipboard lock spans each
    // snapshot/write. Windows CF_UNICODETEXT cannot represent embedded NUL.
    unsafe {
        let owner = CreateWindowExW(
            0,
            windows_sys::w!("STATIC"),
            windows_sys::w!("n2c"),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if owner.is_null() {
            return failure("CreateWindowExW");
        }
        let result = locked(operation, owner);
        DestroyWindow(owner);
        result
    }
}
unsafe fn locked(operation: Operation, owner: HWND) -> ResponseResult {
    unsafe {
        if OpenClipboard(owner) == 0 {
            return failure("OpenClipboard");
        }
        let _open = Open;
        match operation {
            Operation::Capabilities => ResponseResult::Ready,
            Operation::Read => {
                let revision = GetClipboardSequenceNumber() as u64;
                if !CHANGED.swap(false, std::sync::atomic::Ordering::AcqRel)
                    && let Some((sequence, value)) =
                        SNAPSHOT.lock().expect("snapshot cache poisoned").as_ref()
                    && *sequence == revision
                {
                    return ResponseResult::Snapshot {
                        value: value.clone(),
                        revision,
                    };
                }
                if IsClipboardFormatAvailable(UNICODE) == 0 {
                    return ResponseResult::Snapshot {
                        value: if CountClipboardFormats() == 0 {
                            Snapshot::Empty
                        } else {
                            Snapshot::NonText
                        },
                        revision,
                    };
                }
                let handle = GetClipboardData(UNICODE);
                if handle.is_null() {
                    return failure("GetClipboardData");
                }
                let bytes = GlobalSize(handle);
                if !(2..=crate::ipc::MAX_FRAME).contains(&bytes) {
                    return ResponseResult::Permanent;
                }
                let ptr = GlobalLock(handle) as *const u16;
                if ptr.is_null() {
                    return failure("GlobalLock read");
                }
                let units = std::slice::from_raw_parts(ptr, bytes / 2);
                let text = units
                    .iter()
                    .position(|unit| *unit == 0)
                    .and_then(|end| String::from_utf16(&units[..end]).ok());
                GlobalUnlock(handle);
                match text {
                    Some(text) => {
                        let value = Snapshot::Text(text);
                        *SNAPSHOT.lock().expect("snapshot cache poisoned") =
                            Some((revision, value.clone()));
                        ResponseResult::Snapshot { value, revision }
                    }
                    None => ResponseResult::Permanent,
                }
            }
            Operation::Write(text) => {
                *SNAPSHOT.lock().expect("snapshot cache poisoned") = None;
                CHANGED.store(true, std::sync::atomic::Ordering::Release);
                if text.contains('\0') {
                    return ResponseResult::Permanent;
                }
                let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
                if memory.is_null() {
                    return failure("GlobalAlloc");
                }
                let ptr = GlobalLock(memory) as *mut u16;
                if ptr.is_null() {
                    let error = failure("GlobalLock write");
                    GlobalFree(memory);
                    return error;
                }
                std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
                GlobalUnlock(memory);
                if EmptyClipboard() == 0 || SetClipboardData(UNICODE, memory).is_null() {
                    let error = failure("EmptyClipboard/SetClipboardData");
                    GlobalFree(memory);
                    return error;
                }
                ResponseResult::Written
            }
        }
    }
}
