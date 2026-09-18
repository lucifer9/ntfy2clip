use ntfy2clip::protocol::Protocol;

#[test]
fn compatibility_empty_text_invalid_envelopes_and_serialized_byte_limit() {
    let protocol = Protocol::new("test", "e2147baf-fb2c-4e4d-941a-12b9836f7c6b", 4096).unwrap();
    for (raw, expected) in [
        ("", ""),
        ("\\r\\n", ""),
        (" a\\nb\\n", " a\nb"),
        ("abc\\n ", "abc\n "),
        ("{\\\"v\\\":9}", "{\"v\":9}"),
    ] {
        let frame = format!(r#"{{"topic":"test","event":"message","message":"{raw}"}}"#);
        assert_eq!(protocol.decode(&frame).unwrap().as_deref(), Some(expected));
    }
    for frame in [
        r#"{"topic":"test","event":"message","attachment":{},"message":"attachment"}"#,
        r#"{"topic":"test","event":"message","tags":["ntfy2clip"],"message":"bad"}"#,
    ] {
        assert!(protocol.decode(frame).is_err());
    }
    let encoded = protocol.encode("\"\n🙂").unwrap();
    let exact = Protocol::new(
        "test",
        "e2147baf-fb2c-4e4d-941a-12b9836f7c6b",
        encoded.len(),
    )
    .unwrap();
    assert!(exact.encode("\"\n🙂").is_ok());
    assert!(exact.encode("\"\n🙂x").is_err());
}

#[test]
fn decodes_tagged_text_without_losing_whitespace_and_ignores_self() {
    let own = "e2147baf-fb2c-4e4d-941a-12b9836f7c6b";
    let protocol = Protocol::new("test", own, 4096).unwrap();
    let remote = r#"{"event":"message","topic":"test","tags":["ntfy2clip"],"message":"{\"v\":1,\"origin\":\"2b1fda12-6da0-4fcb-b124-a7411e2b83cd\",\"text\":\" 中文🙂 \\r\\n\"}"}"#;
    assert_eq!(
        protocol.decode(remote).unwrap().as_deref(),
        Some(" 中文🙂 ")
    );
    let echo = remote.replace("2b1fda12-6da0-4fcb-b124-a7411e2b83cd", own);
    assert_eq!(protocol.decode(&echo).unwrap(), None);
    let bad = remote.replace("\\\"v\\\":1", "\\\"v\\\":2");
    assert!(protocol.decode(&bad).is_err());
}
