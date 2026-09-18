use ntfy2clip::config::{Config, Mode};
#[test]
fn configuration_is_explicit_and_new_values_are_validated() {
    let cfg = Config::parse(|key| (key == "TOPIC").then(|| "test".into())).unwrap();
    assert_eq!(cfg.mode, Mode::Receive);
    assert_eq!(cfg.max_message, 4096);
    assert_eq!(cfg.http_url.as_str(), "https://ntfy.sh/test");
    for (key, value) in [
        ("SYNC_MODE", "both"),
        ("SEND_QUEUE_MAX_MESSAGES", "0"),
        ("WRITE_TIMEOUT_MS", "oops"),
        ("TOPIC", "a,b"),
        ("SERVER", "user:password@host"),
    ] {
        assert!(
            Config::parse(|name| if name == key {
                Some(value.into())
            } else if name == "TOPIC" {
                Some("test".into())
            } else {
                None
            })
            .is_err(),
            "{key}"
        );
    }
}
