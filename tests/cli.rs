use std::process::Command;
#[test]
fn invalid_configuration_fails_without_leaking_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_n2c"))
        .env_clear()
        .env("DEV", "1")
        .env("TOPIC", "test")
        .env("SYNC_MODE", "invalid")
        .env("TOKEN", "synthetic-secret-must-not-appear")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("SYNC_MODE"));
    assert!(!stderr.contains("synthetic-secret"));
}
