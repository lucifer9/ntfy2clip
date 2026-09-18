use ntfy2clip::ipc::{self, Operation, Request};
#[test]
fn framed_ipc_preserves_boundaries_and_rejects_truncation_or_oversize() {
    let request = Request {
        id: 7,
        budget_ms: 1000,
        operation: Operation::Write("\n中文🙂\0".into()),
    };
    let mut stream = ipc::encode(&request).unwrap();
    stream.extend(
        ipc::encode(&Request {
            id: 8,
            budget_ms: 1000,
            operation: Operation::Read,
        })
        .unwrap(),
    );
    let mut input = &stream[..];
    let decoded: Request = ipc::read_frame(&mut input).unwrap();
    assert_eq!(decoded.id, 7);
    assert!(matches!(decoded.operation,Operation::Write(text) if text == "\n中文🙂\0"));
    assert_eq!(ipc::read_frame::<Request>(&mut input).unwrap().id, 8);
    assert!(ipc::read_frame::<Request>(&mut &stream[..5]).is_err());
    assert!(ipc::read_frame::<Request>(&mut &(u32::MAX.to_be_bytes())[..]).is_err());
}
