#![cfg(unix)]
use ntfy2clip::clipboard::write_command;
use std::time::{Duration, Instant};
use tokio::process::Command;

#[tokio::test]
async fn reports_nonzero_and_reaps_timed_out_writer() {
    let mut fail = Command::new("/bin/sh");
    fail.args(["-c", "exit 7"]);
    assert!(
        write_command(&mut fail, b"synthetic", Duration::from_secs(1))
            .await
            .is_err()
    );
    let mut slow = Command::new("/bin/sleep");
    slow.arg("10");
    let start = Instant::now();
    assert!(
        write_command(&mut slow, b"", Duration::from_millis(20))
            .await
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    let dir = std::env::temp_dir().join(format!("n2c-process-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let pid_file = dir.join("pid");
    let late_file = dir.join("late");
    let mut late = Command::new("/usr/bin/perl");
    late.args(["-e", "open(my $p, '>', $ARGV[0]) or die; print $p $$; close $p; sleep 1; open(my $l, '>', $ARGV[1]) or die; print $l 'late';"]).arg(&pid_file).arg(&late_file);
    assert!(
        write_command(&mut late, b"", Duration::from_millis(200))
            .await
            .is_err()
    );
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "timed-out writer must have been reaped"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!late_file.exists(), "no late write after timeout");
    std::fs::remove_file(pid_file).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
