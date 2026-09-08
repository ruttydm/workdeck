//! Synchronous document callbacks for single-request native extension loops.

use crate::{
    EXTENSION_DOCUMENT_READ_METHOD, ExtensionDocumentReadRequest, ExtensionFileSide,
    JsonRpcRequest, MAX_MESSAGE_BYTES,
};
use serde_json::Value;
use std::io::{self, BufRead, Read, Write};

/// Request one captured source side from the host.
///
/// The caller owns child-ID allocation and must not multiplex other requests on
/// these streams while waiting. Matching parent cancellation interrupts the wait.
/// This helper bounds response allocation, but cannot impose an I/O deadline on
/// arbitrary blocking streams; use a transport with its own deadline if needed.
pub fn read_extension_document(
    input: &mut impl BufRead,
    output: &mut impl Write,
    parent_id: u64,
    child_id: u64,
    side: ExtensionFileSide,
) -> io::Result<Option<String>> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: child_id,
        method: EXTENSION_DOCUMENT_READ_METHOD.into(),
        params: serde_json::to_value(ExtensionDocumentReadRequest {
            parent_request_id: parent_id,
            side,
        })
        .map_err(io::Error::other)?,
    };
    serde_json::to_writer(&mut *output, &request).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()?;
    loop {
        let mut line = Vec::new();
        let count = input
            .take((MAX_MESSAGE_BYTES + 2) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if line.last() != Some(&b'\n') || line.len() > MAX_MESSAGE_BYTES + 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unterminated or oversized document response",
            ));
        }
        let value: Value = serde_json::from_slice(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid JSON-RPC version",
            ));
        }
        if value.get("method").and_then(Value::as_str) == Some("$/cancelRequest")
            && value.get("id").is_none()
        {
            if value.pointer("/params/id").and_then(Value::as_u64) == Some(parent_id) {
                let cancellation: crate::ExtensionRequestCancellation =
                    serde_json::from_value(value["params"].clone())
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                return Err(io::Error::new(io::ErrorKind::Interrupted, cancellation));
            }
            continue;
        }
        if value.get("method").is_some()
            || value.get("id").and_then(Value::as_u64) != Some(child_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected document response",
            ));
        }
        if let Some(error) = value.get("error") {
            if value.get("result").is_some()
                || serde_json::from_value::<crate::JsonRpcError>(error.clone()).is_err()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid document error response",
                ));
            }
            return Err(io::Error::other("host rejected document request"));
        }
        return match value.get("result") {
            Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid document result",
            )),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_preserves_typed_parent_cancellation_metadata() {
        for params in [
            serde_json::json!({"id":8}),
            serde_json::json!({"id":8,"cause":"timed_out","reason":{"message":"deadline","details":[1,true]}}),
        ] {
            let frame =
                serde_json::json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":params});
            let error = read_extension_document(
                &mut io::Cursor::new(format!("{frame}\n")),
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            let cancellation = error
                .get_ref()
                .unwrap()
                .downcast_ref::<crate::ExtensionRequestCancellation>()
                .unwrap();
            assert_eq!(serde_json::to_value(cancellation).unwrap(), params);
        }
    }

    #[test]
    fn callback_rejects_ambiguous_and_malformed_error_responses() {
        for payload in [
            serde_json::json!({"result":null,"error":{"code":-32602,"message":"bad"}}),
            serde_json::json!({"result":"text","error":null}),
            serde_json::json!({"error":null}),
            serde_json::json!({"error":{"code":"bad","message":"bad"}}),
            serde_json::json!({"error":{"code":-32602}}),
        ] {
            let mut frame = payload;
            frame["jsonrpc"] = serde_json::json!("2.0");
            frame["id"] = serde_json::json!(3);
            let mut input = io::Cursor::new(format!("{frame}\n"));
            assert_eq!(
                read_extension_document(&mut input, &mut Vec::new(), 8, 3, ExtensionFileSide::New)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
        let frame = serde_json::json!({"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"rejected"}});
        assert_eq!(
            read_extension_document(
                &mut io::Cursor::new(format!("{frame}\n")),
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Other
        );
    }

    #[test]
    fn callback_enforces_exact_frame_limit_without_draining_oversized_input() {
        let prefix = "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":\"";
        let suffix = "\"}";
        let text_len = MAX_MESSAGE_BYTES - prefix.len() - suffix.len();
        let mut exact = io::Cursor::new(format!("{prefix}{}{suffix}\n", "x".repeat(text_len)));
        let result =
            read_extension_document(&mut exact, &mut Vec::new(), 8, 3, ExtensionFileSide::New)
                .unwrap()
                .unwrap();
        assert_eq!(result.len(), text_len);
        assert_eq!(exact.position(), (MAX_MESSAGE_BYTES + 1) as u64);

        let mut oversized = io::Cursor::new(vec![b'x'; MAX_MESSAGE_BYTES * 2]);
        assert_eq!(
            read_extension_document(
                &mut oversized,
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(oversized.position(), (MAX_MESSAGE_BYTES + 2) as u64);
        let mut over_by_one =
            io::Cursor::new(format!("{prefix}{}{suffix}\n", "x".repeat(text_len + 1)));
        assert_eq!(
            read_extension_document(
                &mut over_by_one,
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn callback_ignores_other_parent_cleanup_and_reports_eof() {
        let frames = "{\"jsonrpc\":\"2.0\",\"method\":\"$/cancelRequest\",\"params\":{\"id\":7}}\n{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":null}\n";
        assert_eq!(
            read_extension_document(
                &mut io::Cursor::new(frames),
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New
            )
            .unwrap(),
            None
        );
        assert_eq!(
            read_extension_document(
                &mut io::Cursor::new(b""),
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn callback_preserves_side_parent_text_and_null() {
        for result in [Value::Null, Value::String("a\nλ".into())] {
            let mut input = io::Cursor::new(format!(
                "{}\n",
                serde_json::json!({"jsonrpc":"2.0","id":3,"result":result})
            ));
            let mut output = Vec::new();
            let actual =
                read_extension_document(&mut input, &mut output, 8, 3, ExtensionFileSide::Old)
                    .unwrap();
            assert_eq!(actual, result.as_str().map(str::to_owned));
            let request: JsonRpcRequest = serde_json::from_slice(&output).unwrap();
            assert_eq!(request.id, 3);
            assert_eq!(
                request.params,
                serde_json::json!({"parentRequestId":8,"side":"old"})
            );
        }
    }

    #[test]
    fn callback_rejects_invalid_frames_and_observes_parent_cancellation() {
        for frame in [
            "{\"jsonrpc\":\"1.0\",\"id\":3,\"result\":null}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":null}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":42}\n",
            "{}",
        ] {
            assert_eq!(
                read_extension_document(
                    &mut io::Cursor::new(frame),
                    &mut Vec::new(),
                    8,
                    3,
                    ExtensionFileSide::New
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::InvalidData
            );
        }
        let frame = "{\"jsonrpc\":\"2.0\",\"method\":\"$/cancelRequest\",\"params\":{\"id\":8}}\n";
        assert_eq!(
            read_extension_document(
                &mut io::Cursor::new(frame),
                &mut Vec::new(),
                8,
                3,
                ExtensionFileSide::New
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
    }
}
