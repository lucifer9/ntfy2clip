use crate::{
    clipboard::Snapshot,
    ipc::{Operation, ResponseResult},
};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSString, NSUTF8StringEncoding};

pub fn operate(operation: Operation) -> ResponseResult {
    // Runs on the helper's main thread. A wedged pasteboard RPC is bounded by
    // terminating the helper, never by abandoning an in-process native call.
    objc2::rc::autoreleasepool(|_| {
        let board = NSPasteboard::generalPasteboard();
        match operation {
            Operation::Capabilities => ResponseResult::Ready,
            Operation::Read => {
                let before = board.changeCount();
                let value = match board.stringForType(unsafe { NSPasteboardTypeString }) {
                    Some(text) => {
                        if text.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                            > crate::ipc::MAX_FRAME
                        {
                            return ResponseResult::Permanent;
                        }
                        Snapshot::Text(text.to_string())
                    }
                    None => match board.types() {
                        Some(types) if types.count() > 0 => Snapshot::NonText,
                        _ => Snapshot::Empty,
                    },
                };
                if before != board.changeCount() {
                    return ResponseResult::Temporary;
                }
                ResponseResult::Snapshot {
                    value,
                    revision: before as u64,
                }
            }
            Operation::Write(text) => {
                board.clearContents();
                if board.setString_forType(&NSString::from_str(&text), unsafe {
                    NSPasteboardTypeString
                }) {
                    ResponseResult::Written
                } else {
                    ResponseResult::Temporary
                }
            }
        }
    })
}
