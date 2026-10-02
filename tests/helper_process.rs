#![cfg(unix)]

use ntfy2clip::{
    clipboard::{Clipboard, Snapshot, WriteError},
    platform::NativeClipboard,
};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

struct Helper {
    directory: PathBuf,
    executable: PathBuf,
}
impl Helper {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("n2c-helper-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let executable = directory.join("clipboard-helper");
        fs::write(&executable, include_bytes!("fixtures/clipboard_helper.pl")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            directory,
            executable,
        }
    }

    fn assert_all_children_reaped(&self) {
        let pids = fs::read_to_string(self.executable.with_extension("pids")).unwrap();
        assert!(!pids.is_empty());
        for pid in pids.lines() {
            let pid: i32 = pid.parse().unwrap();
            // Only probe our fake helper's PID; never signal a user's process.
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "helper {pid} is still alive"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).expect("remove test helper directory");
    }
}

#[tokio::test]
async fn helper_round_trips_unicode_and_empty_text_then_shuts_down() {
    let helper = Helper::new();
    let mut clipboard = NativeClipboard::new(helper.executable.clone());
    clipboard.connect().await.unwrap();
    for text in [" 中文🙂 \r\n", ""] {
        clipboard.write(text, Duration::from_secs(1)).await.unwrap();
        assert_eq!(clipboard.read().await.unwrap(), Snapshot::Text(text.into()));
    }
    clipboard.shutdown().await.unwrap();
    helper.assert_all_children_reaped();
}

#[tokio::test]
async fn failed_helper_exchanges_reap_the_child_and_next_read_recovers() {
    for fault in ["exit", "timeout", "id", "version", "oversize"] {
        let helper = Helper::new();
        let mut clipboard = NativeClipboard::new(helper.executable.clone());
        clipboard.connect().await.unwrap();
        let result = clipboard
            .write(&format!("fault:{fault}"), Duration::from_millis(100))
            .await;
        assert!(
            matches!(result, Err(WriteError::Temporary(_))),
            "{fault}: {result:?}"
        );
        helper.assert_all_children_reaped();
        assert_eq!(
            clipboard.read().await.unwrap(),
            Snapshot::Text("fresh helper".into()),
            "{fault}"
        );
        clipboard.shutdown().await.unwrap();
        helper.assert_all_children_reaped();
    }
}
