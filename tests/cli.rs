use std::process::Command;
#[cfg(target_os = "linux")]
#[test]
fn wsl_is_rejected_instead_of_using_wslg() {
    let output = Command::new(env!("CARGO_BIN_EXE_n2c"))
        .env_clear()
        .env("TOPIC", "test")
        .env("SYNC_MODE", "bidirectional")
        .env("WSL_DISTRO_NAME", "synthetic-wsl")
        .env("WAYLAND_DISPLAY", "wayland-0")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Windows and WSL are not supported")
    );
}

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
